use clap::Parser;
use env_logger::Env;
use log::info;

use tokio::sync::mpsc;

use blackmagic_camera_bridge::{
    ble::{CameraController, MockCameraController},
    events::{EventDispatcher, RecordingEvent},
    osc::OscSender,
};

#[derive(Parser, Debug)]
#[command(name = "camlink", version, about = "Bridge BMPCC4K record signals to REAPER via OSC")]
struct Args {
    /// Camera Bluetooth MAC address
    #[arg(short, long)]
    camera: Option<String>,

    /// OSC target host
    #[arg(long, default_value = "127.0.0.1")]
    osc_host: String,

    /// OSC target port
    #[arg(long, default_value_t = 6666)]
    osc_port: u16,

    /// Use mock camera instead of real hardware
    #[arg(long, default_value_t = false)]
    mock: bool,
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    info!("Starting camlink...");
    info!("OSC target: {}:{}", args.osc_host, args.osc_port);

    let mut controller: Box<dyn CameraController> = if args.mock {
        info!("Using mock camera controller");
        Box::new(MockCameraController::new())
    } else {
        // TODO: Create real btleplug controller
        todo!("Real camera controller implementation");
    };

    let osc = OscSender::connect(&args.osc_host, args.osc_port)
    .await
    .expect("Failed to create OSC sender socket");
    let dispatcher = EventDispatcher::new(osc);

    let (tx, rx) = mpsc::channel::<RecordingEvent>(100);
    controller
    .subscribe_to_record_notifications(tx)
    .await
    .expect("Failed to subscribe to camera events");

    tokio::spawn(async move {
        dispatcher.run(rx).await;
    });

    info!("camlink ready. Press Ctrl+C to exit.");

    // Keep running indefinitely
    tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    info!("Shutting down...");
}
