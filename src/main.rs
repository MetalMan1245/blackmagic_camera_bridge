use clap::Parser;
use env_logger::Env;
use log::{error, info};
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;

use blackmagic_camera_bridge::{
    ble::{coral_impl::CoralCameraController, CameraController, MockCameraController},
    config::Config,
    events::{EventDispatcher, RecordingEvent},
    osc::OscSender,
};

#[derive(Parser, Debug)]
#[command(
    name = "camlink",
    version,
    about = "Bridge BMPCC4K record signals to REAPER via OSC"
)]
struct Args {
    /// Camera Bluetooth MAC address
    #[arg(short, long)]
    camera: Option<String>,

    /// OSC target host
    #[arg(long)]
    osc_host: Option<String>,

    /// OSC target port
    #[arg(long)]
    osc_port: Option<u16>,

    /// Use mock camera instead of real hardware
    #[arg(long)]
    mock: bool,

    /// Scan for nearby Blackmagic cameras and exit
    #[arg(long)]
    scan: bool,

    /// Config file path
    #[arg(long, default_value = "~/.config/camlink/config.json")]
    config: String,
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    info!("Starting camlink...");

    // Load config from file
    let config_path = expand_tilde(&args.config);
    let config = match Config::load(&config_path) {
        Ok(c) => {
            info!("Loaded config from {}", config_path.display());
            c
        }
        Err(e) => {
            log::warn!(
                "Could not load config from {}: {}",
                config_path.display(),
                e
            );
            Config::default()
        }
    };

    // CLI arguments override config
    let camera_address = args.camera.or_else(|| {
        if !config.camera.mac_address.is_empty() {
            Some(config.camera.mac_address.clone())
        } else {
            None
        }
    });

    let osc_host = args.osc_host.unwrap_or(config.osc.target_host);
    let osc_port = args.osc_port.unwrap_or(config.osc.target_port);

    info!("OSC target: {}:{} ", osc_host, osc_port);
    info!("Camera address: {:?}", camera_address);

    // --scan: list nearby Blackmagic cameras and exit.
    if args.scan {
        let controller = CoralCameraController::new()
            .await
            .expect("failed to initialise BLE adapter for scan");
        info!("Scanning for Blackmagic cameras (5s)...");
        match controller.discover().await {
            Ok(devices) if devices.is_empty() => {
                info!("No Blackmagic cameras found. Is the camera powered on with Bluetooth enabled?");
            }
            Ok(devices) => {
                info!("Found {} camera(s):", devices.len());
                for (addr, name) in devices {
                    println!("{addr}  {name}");
                }
            }
            Err(e) => {
                error!("Scan failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    let mut controller: Box<dyn CameraController> = if args.mock {
        info!("Using mock camera controller");
        Box::new(MockCameraController::new())
    } else {
        let Some(mac) = camera_address.as_deref() else {
            error!("Camera MAC address required for non-mock mode; provide via --camera or config file (or use --scan to find cameras)");
            std::process::exit(1);
        };
        info!("Using real BLE camera controller");
        let ctrl = match CoralCameraController::new().await {
            Ok(c) => c,
            Err(e) => {
                error!("Could not initialise BLE adapter: {e}");
                error!("Hint: check that bluetoothd is running and your adapter is powered on (bluetoothctl show). Use --mock to test without hardware.");
                std::process::exit(1);
            }
        };
        Box::new(ctrl)
    };

    let osc = OscSender::connect(&osc_host, osc_port)
        .await
        .expect("Failed to create OSC sender socket");
    let dispatcher = EventDispatcher::new(osc);

    let (tx, rx) = mpsc::channel::<RecordingEvent>(100);

    if !args.mock {
        // Real hardware path: connect with bounded retries before subscribing.
        let mac = camera_address.as_deref().expect("checked above");
        if let Err(e) = blackmagic_camera_bridge::ble::connect_with_retry(
            controller.as_mut(),
            mac,
            3,
            Duration::from_millis(500),
        )
        .await
        {
            error!("Could not connect to camera at {mac}: {e}");
            std::process::exit(1);
        }
    }

    controller
        .subscribe_to_record_notifications(tx)
        .await
        .expect("Failed to subscribe to camera events");

    tokio::spawn(async move {
        dispatcher.run(rx).await;
    });

    info!("camlink ready. Press Ctrl+C to exit.");

    // Keep running indefinitely, handle graceful shutdown
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    info!("Shutdown signal received...");

    controller
        .disconnect()
        .await
        .expect("Failed to disconnect from camera");
    info!("Disconnected from camera");
    info!("Shutdown complete");
}
