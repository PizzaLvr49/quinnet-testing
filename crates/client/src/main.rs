use bevy::prelude::*;
use bevy_enhanced_input::EnhancedInputPlugin;
use bevy_panic_handler::PanicHandlerBuilder;
use bevy_quinnet::client::{
    ClientConnectionConfiguration, ClientConnectionConfigurationDefaultables, QuinnetClient,
    certificate::CertificateVerificationMode,
    connection::{ClientAddrConfiguration, ConnectionEvent},
};
use bevy_replicon::prelude::*;
use bevy_replicon_quinnet::{ChannelsConfigurationExt, RepliconQuinnetPlugins};
use clap::Parser;
use std::net::{IpAddr, Ipv6Addr};

#[derive(Resource, Parser)]
struct Args {
    #[arg(short, long, default_value_t = Ipv6Addr::LOCALHOST.into())]
    ip: IpAddr,
    #[arg(short, long, default_value_t = 5000)]
    port: u16,
}

fn main() {
    let args = Args::parse();

    let mut app = App::new();
    app.insert_resource(args);

    configure_plugins(&mut app);
    configure_systems(&mut app);
    configure_replication(&mut app);

    app.run();
}

fn configure_plugins(app: &mut App) {
    app.add_plugins(DefaultPlugins)
        .add_plugins((EnhancedInputPlugin, PanicHandlerBuilder::default().build()))
        .add_plugins((RepliconPlugins, RepliconQuinnetPlugins));
}

fn configure_replication(_app: &mut App) {}

fn configure_systems(app: &mut App) {
    app.add_systems(Startup, setup_client);
    app.add_systems(Update, read_connected);
    app.add_systems(Last, disconnect_observer);
}

fn read_connected(mut reader: MessageReader<ConnectionEvent>) {
    for message in reader.read() {
        info!("Client Id is: {}", message.client_id.unwrap())
    }
}

fn setup_client(
    args: Res<Args>,
    channels: Res<RepliconChannels>,
    mut client: ResMut<QuinnetClient>,
    mut commands: Commands,
) {
    let (ip, port) = (args.ip, args.port);

    client
        .open_connection(ClientConnectionConfiguration {
            addr_config: ClientAddrConfiguration::from_ips(ip, port, Ipv6Addr::UNSPECIFIED, 0),
            cert_mode: CertificateVerificationMode::SkipVerification,
            defaultables: ClientConnectionConfigurationDefaultables {
                send_channels_cfg: channels.client_configs(),
            },
        })
        .unwrap();

    info!("Client connecting to [{ip}]:{port}");

    commands.spawn(Camera2d);
}

fn disconnect_observer(mut exit_events: MessageReader<AppExit>, mut client: ResMut<QuinnetClient>) {
    for _event in exit_events.read() {
        info!("Disconnecting all connections...");
        let connection_ids: Vec<u64> = client.connections().map(|(id, _)| *id).collect();

        for connection_id in connection_ids {
            if let Err(e) = client.close_connection(connection_id) {
                warn!("Failed to close connection {}: {:?}", connection_id, e);
            }
        }
    }
}
