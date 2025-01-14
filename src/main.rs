use anyhow::Result;
use clap::Parser;
use k8s_openapi::api::coordination::v1 as coordv1;
use k8s_openapi::api::coordination::v1::Lease;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{
    api::{Api, ListParams, PatchParams, ResourceExt},
    Client,
};
use kubert::lease::LeaseManager;
use tokio::sync::oneshot;
use tokio::time::Duration;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{filter::EnvFilter, fmt};

const LEASE_NAME: &str = "lease-test";
const LEASE_DURATION: Duration = Duration::from_secs(30);
const RENEW_GRACE_PERIOD: Duration = Duration::from_secs(1);

mod cli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    // setup logging
    let level_filter = cli.log_level;
    let filter_layer = EnvFilter::from_default_env()
        .add_directive(level_filter.into())
        .add_directive("rustls=off".parse().unwrap()) // this crate generates tracing events we don't care about
        .add_directive("hyper=off".parse().unwrap()) // this crate generates tracing events we don't care about
        .add_directive("tower=off".parse().unwrap()); // this crate generates tracing events we don't care about
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();

    let client = Client::try_default().await?;

    let lease_manager = init_lease(client.clone(), "default", "not relevant").await?;

    let leases: Api<Lease> = Api::namespaced(client, "default");
    for l in leases.list(&ListParams::default()).await? {
        tracing::debug!("found lease {}", l.name_any());
    }

    let params = kubert::lease::ClaimParams {
        lease_duration: LEASE_DURATION,
        renew_grace_period: RENEW_GRACE_PERIOD,
    };

    let (mut claims, _task) = lease_manager.spawn(cli.claimant.clone(), params).await?;

    tracing::debug!("waiting to be leader");
    claims
        .wait_for(|receiver| receiver.is_current_for(&cli.claimant))
        .await
        .unwrap();

    let worker = tokio::spawn(async move {
        //let _start = job_start_rx.await.unwrap();
        tracing::debug!("starting job");
        for i in 0..10 {
            tracing::debug!("{i} awake");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    worker.await.unwrap();

    Ok(())
}

async fn init_lease(client: Client, ns: &str, deployment_name: &str) -> Result<LeaseManager> {
    // Fetch the policy-controller deployment so that we can use it as an owner
    // reference of the Lease.
    //let api = kube::Api::<Deployment>::namespaced(client.clone(), ns);
    //let deployment = api.get(deployment_name).await?;

    let api = kube::Api::namespaced(client, ns);
    let params = PatchParams {
        field_manager: Some("policy-controller".to_string()),
        ..Default::default()
    };
    match api
        .patch(
            LEASE_NAME,
            &params,
            &kube::api::Patch::Apply(coordv1::Lease {
                metadata: ObjectMeta {
                    name: Some(LEASE_NAME.to_string()),
                    namespace: Some(ns.to_string()),
                    // Specifying a resource version of "0" means that we will
                    // only create the Lease if it does not already exist.
                    resource_version: Some("0".to_string()),
                    //owner_references: Some(vec![deployment.controller_owner_ref(&()).unwrap()]),
                    labels: Some(
                        [
                            (
                                "linkerd.io/control-plane-component".to_string(),
                                "destination".to_string(),
                            ),
                            ("linkerd.io/control-plane-ns".to_string(), ns.to_string()),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                    ..Default::default()
                },
                spec: None,
            }),
        )
        .await
    {
        Ok(lease) => tracing::info!(?lease, "created Lease resource"),
        Err(kube::Error::Api(_)) => tracing::info!("Lease already exists, no need to create it"),
        Err(error) => {
            tracing::error!(%error, "error creating Lease resource");
            return Err(error.into());
        }
    };
    // Create the lease manager used for trying to claim the policy
    // controller write lease.
    // todo: Do we need to use LeaseManager::field_manager here?
    kubert::lease::LeaseManager::init(api, LEASE_NAME)
        .await
        .map_err(Into::into)
}
