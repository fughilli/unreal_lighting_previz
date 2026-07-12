//! Reassembles decoded Art-Net universes into a flat voxel volume.
//!
//! The mapping is a direct port of `VolumetricDisplay::listenArtNet` in
//! `artnet-tester` (the ground-truth C++ simulator), so the previz lights the
//! exact voxel the hardware would. The published buffer is in canonical
//! `(z, y, x)` raster order per cube — matching the Python `Raster`'s
//! `(length, height, width, 3)` layout — with cubes concatenated in config
//! order.

use crate::artnet::Packet;
use crate::config::{Config, Cube, Listener};

pub type Rgb = [u8; 3];

/// Number of RGB pixels carried by one full 510-channel universe.
pub const PIXELS_PER_UNIVERSE: usize = 170;

/// Default universes per z-layer. The hardware sender and C++ simulator both
/// use 3 (enough for a 20×20 = 400-pixel cross-section: ceil(400/170) = 3).
pub const DEFAULT_UNIVERSES_PER_LAYER: usize = 3;

pub struct Volume {
    cubes: Vec<Cube>,
    universes_per_layer: usize,
    /// Frame currently being filled by incoming ArtDmx packets.
    work: Vec<Rgb>,
}

impl Volume {
    pub fn new(config: &Config, universes_per_layer: usize) -> Volume {
        Volume {
            cubes: config.cubes.clone(),
            universes_per_layer,
            work: vec![[0, 0, 0]; config.num_voxels],
        }
    }

    pub fn num_voxels(&self) -> usize {
        self.work.len()
    }

    /// The in-progress frame buffer, `(z, y, x)` order, cubes concatenated.
    pub fn work(&self) -> &[Rgb] {
        &self.work
    }

    /// Apply one decoded packet for a given listener. `Sync` is a no-op here —
    /// the caller decides when to publish (latch) the `work` buffer.
    pub fn apply(&mut self, listener: &Listener, packet: &Packet) {
        if let Packet::Dmx { universe, data } = packet {
            self.apply_dmx(listener, *universe, data);
        }
    }

    /// Port of the C++ receiver's per-packet write loop.
    fn apply_dmx(&mut self, listener: &Listener, universe: u16, data: &[u8]) {
        let upl = self.universes_per_layer;
        let universe = universe as usize;

        // Which entry in *this listener's* z_indices does the universe address?
        let layer = universe / upl;
        let actual_z = match listener.z_indices.get(layer) {
            Some(&z) => z,
            // Universe addresses a layer this controller doesn't own; drop it
            // (the C++ code logs a warning and continues).
            None => return,
        };

        let universe_in_layer = universe % upl;
        let start_pixel_in_layer = universe_in_layer * PIXELS_PER_UNIVERSE;

        let cube = &self.cubes[listener.cube_index];
        let (w, h) = (cube.width, cube.height);
        let layer_pixels = w * h;
        let layer_base = cube.offset + actual_z * w * h;

        for (j, chunk) in data.chunks_exact(3).enumerate() {
            let idx_in_layer = start_pixel_in_layer + j;
            if idx_in_layer >= layer_pixels {
                continue; // overflow past this layer's pixels
            }
            let x = idx_in_layer % w;
            let y = idx_in_layer / w;
            let pixel_index = layer_base + x + y * w;
            // `pixel_index < len` always holds given the bounds above, but keep
            // the guard to mirror the C++ defensively.
            if let Some(slot) = self.work.get_mut(pixel_index) {
                *slot = [chunk[0], chunk[1], chunk[2]];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artnet;
    use crate::config::Config;

    /// Reproduce `artnet-tester`'s sender packetization for one loopback
    /// controller that owns z-layers `0..length` (the dev/loopback path), then
    /// assert the receiver reconstructs the exact source raster.
    fn round_trip(w: usize, h: usize, l: usize) {
        let cfg_json = format!(
            r#"{{"world_geometry":"{w}x{h}x{l}","cubes":[{{
                "position":[0,0,0],"dimensions":"{w}x{h}x{l}",
                "artnet_mappings":[{{"ip":"127.0.0.1","port":6454,
                  "z_idx":{:?}}}]}}]}}"#,
            (0..l).collect::<Vec<_>>()
        );
        let cfg = Config::from_json_str(&cfg_json).unwrap();
        let listener = cfg.listeners[0].clone();

        // Source raster in (z,y,x) order with a distinctive pattern.
        let mut src = vec![[0u8; 3]; w * h * l];
        for z in 0..l {
            for y in 0..h {
                for x in 0..w {
                    src[z * w * h + y * w + x] = [
                        (z * 7 + 1) as u8,
                        (y * 3 + 2) as u8,
                        (x * 5 + 3) as u8,
                    ];
                }
            }
        }

        // Serialize exactly like sender.py + core.rs: per z-layer, flatten
        // (y,x) row-major, split into 510-byte universes starting at z*upl.
        let upl = DEFAULT_UNIVERSES_PER_LAYER;
        let mut volume = Volume::new(&cfg, upl);
        for z in 0..l {
            let mut layer_bytes = Vec::with_capacity(w * h * 3);
            for y in 0..h {
                for x in 0..w {
                    layer_bytes.extend_from_slice(&src[z * w * h + y * w + x]);
                }
            }
            let mut universe = (z * upl) as u16;
            for chunk in layer_bytes.chunks(510) {
                let pkt = make_dmx(universe, chunk);
                let parsed = artnet::parse(&pkt).unwrap();
                volume.apply(&listener, &parsed);
                universe += 1;
            }
        }

        assert_eq!(volume.work(), src.as_slice(), "round-trip mismatch for {w}x{h}x{l}");
    }

    fn make_dmx(universe: u16, data: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(artnet::ARTNET_ID);
        p.extend_from_slice(&artnet::OP_DMX.to_le_bytes());
        p.extend_from_slice(&14u16.to_be_bytes());
        p.push(0);
        p.push(0);
        p.extend_from_slice(&universe.to_le_bytes());
        p.extend_from_slice(&(data.len() as u16).to_be_bytes());
        p.extend_from_slice(data);
        p
    }

    #[test]
    fn round_trip_20x20x4() {
        round_trip(20, 20, 4);
    }

    #[test]
    fn round_trip_full_cross_section() {
        // 20x20 layer => 1200 bytes => 3 universes (510,510,180), exercising the
        // universe_in_layer / start_pixel boundary math.
        round_trip(20, 20, 2);
    }

    #[test]
    fn round_trip_napa_slice() {
        // A few layers of the real 20x20x140 cross-section.
        round_trip(20, 20, 8);
    }

    #[test]
    fn single_voxel_lights_expected_index() {
        // plane_scene-style: light voxel (x=1,y=2,z=3) red, nothing else.
        let cfg = Config::from_json_str(
            r#"{"world_geometry":"20x20x20","cubes":[{"position":[0,0,0],
               "dimensions":"20x20x20","artnet_mappings":[{"ip":"127.0.0.1",
               "port":6454,"z_idx":[0,1,2,3,4,5]}]}]}"#,
        )
        .unwrap();
        let listener = cfg.listeners[0].clone();
        let mut volume = Volume::new(&cfg, DEFAULT_UNIVERSES_PER_LAYER);

        // z=3 -> base universe 9; pixel (x=1,y=2) is index 41 in the layer,
        // which lands in universe_in_layer 0 (pixels 0..169), channel 41*3.
        let (w, x, y, z) = (20usize, 1usize, 2usize, 3usize);
        let idx_in_layer = y * w + x; // 41
        let mut layer = vec![0u8; 170 * 3];
        layer[idx_in_layer * 3] = 255;
        let pkt = make_dmx((z * 3) as u16, &layer);
        volume.apply(&listener, &artnet::parse(&pkt).unwrap());

        let expected_index = z * w * w + y * w + x;
        for (i, &px) in volume.work().iter().enumerate() {
            if i == expected_index {
                assert_eq!(px, [255, 0, 0]);
            } else {
                assert_eq!(px, [0, 0, 0], "voxel {i} should be dark");
            }
        }
    }

    #[test]
    fn drops_universe_for_unowned_layer() {
        let cfg = Config::from_json_str(
            r#"{"world_geometry":"20x20x20","cubes":[{"position":[0,0,0],
               "dimensions":"20x20x20","artnet_mappings":[{"ip":"127.0.0.1",
               "port":6454,"z_idx":[0,1]}]}]}"#,
        )
        .unwrap();
        let listener = cfg.listeners[0].clone();
        let mut volume = Volume::new(&cfg, DEFAULT_UNIVERSES_PER_LAYER);
        // layer index 5 (universe 15) is beyond this listener's 2 z_indices.
        let pkt = make_dmx(15, &[255u8; 9]);
        volume.apply(&listener, &artnet::parse(&pkt).unwrap());
        assert!(volume.work().iter().all(|&p| p == [0, 0, 0]));
    }
}
