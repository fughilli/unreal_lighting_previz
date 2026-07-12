//! Napa previz volume receiver.
//!
//! A loopback Art-Net sidecar: it binds one UDP socket per virtual controller
//! (per `artnet-tester`'s `config.json`), reassembles the ~56k-voxel volume
//! using the same mapping as the hardware/C++ simulator, latches each frame on
//! ArtSync, and publishes it to POSIX shared memory for Unreal Engine.
//!
//! See `README.md` §6.1 for the architecture and the companion repo
//! `~/Projects/artnet-tester` for the protocol source of truth.

pub mod artnet;
pub mod config;
pub mod shmem;
pub mod volume;
