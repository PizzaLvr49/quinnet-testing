use bevy::prelude::*;
use bevy_quinnet::{
    client::{
        ClientConnectionConfiguration, ClientConnectionConfigurationDefaultables, QuinnetClient,
        certificate::CertificateVerificationMode, connection::ClientAddrConfiguration,
    },
    server::{
        EndpointAddrConfiguration, QuinnetServer, ServerEndpointConfiguration,
        ServerEndpointConfigurationDefaultables, certificate::CertificateRetrievalMode,
    },
};
use bevy_replicon::prelude::*;
use bevy_replicon_quinnet::RepliconQuinnetPlugins;
use clap::Parser;
use std::net::{IpAddr, Ipv6Addr};

#[derive(Parser, Resource, Debug, Clone)]
enum Mode {
    Local,

    Server {
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
    },
    Client {
        #[arg(short, long, default_value_t = Ipv6Addr::LOCALHOST.into())]
        ip: IpAddr,
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
    },
}

fn main() {
    App::new()
        .insert_resource(Mode::parse())
        .add_plugins((DefaultPlugins, RepliconPlugins, RepliconQuinnetPlugins))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mode: Res<Mode>, mut server: ResMut<QuinnetServer>, mut client: ResMut<QuinnetClient>) {
    info!("Starting {:?}", *mode);

    match &*mode {
        Mode::Server { port } => {
            server
                .start_endpoint(ServerEndpointConfiguration {
                    addr_config: EndpointAddrConfiguration::from_ip(
                        IpAddr::V6(Ipv6Addr::LOCALHOST),
                        *port,
                    ),
                    cert_mode: CertificateRetrievalMode::GenerateSelfSigned {
                        server_hostname: Ipv6Addr::LOCALHOST.to_string(),
                    },
                    defaultables: ServerEndpointConfigurationDefaultables::default(),
                })
                .unwrap();
        }
        Mode::Client { ip, port } => {
            client
                .open_connection(ClientConnectionConfiguration {
                    addr_config: ClientAddrConfiguration::from_ips(
                        *ip,
                        *port,
                        Ipv6Addr::UNSPECIFIED,
                        0,
                    ),
                    cert_mode: CertificateVerificationMode::SkipVerification,
                    defaultables: ClientConnectionConfigurationDefaultables::default(),
                })
                .unwrap();

            info!("Client connecting to [{ip}]:{port}");
        }
        Mode::Local => {
            info!("Skipped Networking");
        }
    }
}
