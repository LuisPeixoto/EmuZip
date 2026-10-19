//! Cliente gRPC do Android Emulator (protos do SDK, Apache-2.0).
#![allow(clippy::all)]

pub mod android {
    pub mod emulation {
        pub mod control {
            tonic::include_proto!("android.emulation.control");
        }
        pub mod stats {
            tonic::include_proto!("android.emulation.stats");
        }
    }
}
pub mod emulator_snapshot {
    tonic::include_proto!("emulator_snapshot");
}
pub use android::emulation::{control, stats};

use anyhow::Result;
use control::emulator_controller_client::EmulatorControllerClient;
use control::snapshot_service_client::SnapshotServiceClient;
use tonic::metadata::MetadataValue;
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, Endpoint};

#[derive(Clone)]
pub struct Auth(Option<MetadataValue<tonic::metadata::Ascii>>);

impl tonic::service::Interceptor for Auth {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(v) = &self.0 {
            req.metadata_mut().insert("authorization", v.clone());
        }
        Ok(req)
    }
}

pub type Controller = EmulatorControllerClient<InterceptedService<Channel, Auth>>;
pub type Snapshots = SnapshotServiceClient<InterceptedService<Channel, Auth>>;
pub type Stats = stats::emulator_stats_client::EmulatorStatsClient<InterceptedService<Channel, Auth>>;

#[derive(Clone)]
pub struct Emulator {
    pub ctl: Controller,
    pub snap: Snapshots,
    pub stats: Stats,
}

impl Emulator {
    pub async fn connect(port: u16, token: Option<&str>) -> Result<Self> {
        let ch = Endpoint::from_shared(format!("http://127.0.0.1:{port}"))?.connect_timeout(std::time::Duration::from_secs(3)).connect().await?;
        let auth = Auth(token.filter(|t| !t.is_empty()).map(|t| format!("Bearer {t}").parse()).transpose()?);
        Ok(Self {
            ctl: EmulatorControllerClient::with_interceptor(ch.clone(), auth.clone()).max_decoding_message_size(64 * 1024 * 1024),
            snap: SnapshotServiceClient::with_interceptor(ch.clone(), auth.clone()),
            stats: stats::emulator_stats_client::EmulatorStatsClient::with_interceptor(ch, auth),
        })
    }
}
