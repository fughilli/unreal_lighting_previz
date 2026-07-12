//! `previz-receiver` — binds a UDP socket per controller, reassembles the
//! volume, and publishes each ArtSync-latched frame to shared memory.
//!
//! Usage:
//!   previz-receiver --config <path/to/config.json> [--shm previz_volume]
//!                   [--universes-per-layer 3] [--quiet]
//!
//! Drive it with a known scene from the companion repo, e.g.:
//!   bazelisk run //:sender -- --config sim_config_4.json --scene rainbow_scene.py

use std::net::UdpSocket;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use previz_receiver::config::Config;
use previz_receiver::shmem::{FrameFormat, ShmemWriter};
use previz_receiver::volume::{Volume, DEFAULT_UNIVERSES_PER_LAYER};
use previz_receiver::artnet;

struct Args {
    config: String,
    shm: String,
    universes_per_layer: usize,
    quiet: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut config = None;
    let mut shm = "previz_volume".to_string();
    let mut upl = DEFAULT_UNIVERSES_PER_LAYER;
    let mut quiet = false;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--config" => config = Some(it.next().ok_or("--config needs a value")?),
            "--shm" => shm = it.next().ok_or("--shm needs a value")?,
            "--universes-per-layer" => {
                upl = it
                    .next()
                    .ok_or("--universes-per-layer needs a value")?
                    .parse()
                    .map_err(|_| "--universes-per-layer must be an integer")?;
            }
            "--quiet" => quiet = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(Args {
        config: config.ok_or("--config is required")?,
        shm,
        universes_per_layer: upl,
        quiet,
    })
}

fn print_usage() {
    eprintln!(
        "previz-receiver --config <config.json> [--shm previz_volume] \
         [--universes-per-layer 3] [--quiet]"
    );
}

/// Resolve a possibly-relative path against the directory the user actually
/// invoked from. Under `bazel run`, cwd is the runfiles tree, but Bazel sets
/// `BUILD_WORKING_DIRECTORY` to the real invocation dir — so relative paths like
/// `configs/dev_20x20x20.json` work the same via `bazel run` and the raw binary.
fn resolve_path(p: &str) -> String {
    let path = std::path::Path::new(p);
    if path.is_absolute() {
        return p.to_string();
    }
    match std::env::var("BUILD_WORKING_DIRECTORY") {
        Ok(wd) if !wd.is_empty() => std::path::Path::new(&wd)
            .join(path)
            .to_string_lossy()
            .into_owned(),
        _ => p.to_string(),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let config = Config::from_file(&resolve_path(&args.config))?;

    let (w, h, l) = config.world;
    let format = FrameFormat {
        num_voxels: config.num_voxels,
        width: w as u32,
        height: h as u32,
        length: l as u32,
        cube_count: config.cubes.len() as u32,
    };
    let writer = Arc::new(ShmemWriter::create(&args.shm, format)?);
    let volume = Arc::new(Mutex::new(Volume::new(&config, args.universes_per_layer)));
    let frames = Arc::new(AtomicU64::new(0));

    println!(
        "previz-receiver: {} voxels, world {w}x{h}x{l}, {} cube(s), {} listener(s) -> shm /{}",
        config.num_voxels,
        config.cubes.len(),
        config.listeners.len(),
        args.shm
    );

    let mut handles = Vec::new();
    for listener in &config.listeners {
        let addr = format!("{}:{}", listener.ip, listener.port);
        let socket = UdpSocket::bind(&addr).map_err(|e| format!("bind {addr}: {e}"))?;
        if !args.quiet {
            println!(
                "  listening on {addr} -> cube {} z_indices {:?}",
                listener.cube_index, listener.z_indices
            );
        }

        let listener = listener.clone();
        let volume = Arc::clone(&volume);
        let writer = Arc::clone(&writer);
        let frames = Arc::clone(&frames);
        handles.push(thread::spawn(move || {
            listener_loop(socket, listener, volume, writer, frames);
        }));
    }

    if !args.quiet {
        // Lightweight liveness reporter.
        let frames = Arc::clone(&frames);
        thread::spawn(move || {
            let mut last = 0u64;
            loop {
                thread::sleep(std::time::Duration::from_secs(2));
                let now = frames.load(Ordering::Relaxed);
                if now != last {
                    println!("  published {} frames ({} fps)", now, (now - last) / 2);
                    last = now;
                }
            }
        });
    }

    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

fn listener_loop(
    socket: UdpSocket,
    listener: previz_receiver::config::Listener,
    volume: Arc<Mutex<Volume>>,
    writer: Arc<ShmemWriter>,
    frames: Arc<AtomicU64>,
) {
    let mut buf = [0u8; 1024];
    loop {
        let n = match socket.recv(&mut buf) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let Some(packet) = artnet::parse(&buf[..n]) else {
            continue;
        };
        match packet {
            artnet::Packet::Dmx { .. } => {
                volume.lock().unwrap().apply(&listener, &packet);
            }
            artnet::Packet::Sync => {
                // Latch: publish the whole accumulated volume.
                let guard = volume.lock().unwrap();
                writer.publish(guard.work());
                frames.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
