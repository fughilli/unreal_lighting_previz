//! Parses `artnet-tester`'s `config.json` into the geometry + per-listener
//! mapping the receiver needs.
//!
//! This is deliberately a *consumer* of the companion repo's config format —
//! the previz never invents its own pixel-map (see README §1). We read the same
//! file the real hardware sender reads so both interpret Art-Net identically.

use serde::Deserialize;

/// One physical cube (a contiguous voxel volume) in the display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cube {
    pub width: usize,
    pub height: usize,
    pub length: usize,
    /// Flat voxel index where this cube begins in the published volume buffer.
    /// Cubes are concatenated in config order, matching the C++ simulator's
    /// `pixel_buffer_offset` accumulation in `VolumetricDisplay::listenArtNet`.
    pub offset: usize,
}

impl Cube {
    pub fn voxel_count(&self) -> usize {
        self.width * self.height * self.length
    }
}

/// One Art-Net listener: a UDP socket bound to `ip:port` that feeds a set of
/// z-layers (`z_indices`) into a single cube.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub ip: String,
    pub port: u16,
    pub cube_index: usize,
    pub z_indices: Vec<usize>,
}

/// Fully resolved receiver configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub cubes: Vec<Cube>,
    pub listeners: Vec<Listener>,
    /// Total voxels across all cubes — the size of the published buffer.
    pub num_voxels: usize,
    /// World bounding geometry (w, h, l) as declared by `world_geometry`.
    pub world: (usize, usize, usize),
}

impl Config {
    pub fn from_json_str(s: &str) -> Result<Config, String> {
        let raw: RawConfig =
            serde_json::from_str(s).map_err(|e| format!("invalid config JSON: {e}"))?;
        raw.resolve()
    }

    pub fn from_file(path: &str) -> Result<Config, String> {
        let s = std::fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?;
        Config::from_json_str(&s)
    }
}

// --- Raw (on-disk) shapes -----------------------------------------------------

#[derive(Deserialize)]
struct RawConfig {
    world_geometry: String,
    cubes: Vec<RawCube>,
}

#[derive(Deserialize)]
struct RawCube {
    dimensions: String,
    #[serde(default)]
    artnet_mappings: Vec<RawMapping>,
}

#[derive(Deserialize)]
struct RawMapping {
    ip: String,
    /// Ports are sometimes strings, sometimes ints in these configs.
    #[serde(deserialize_with = "de_port")]
    port: u16,
    z_idx: Vec<usize>,
}

fn de_port<'de, D>(d: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::String(s) => s.parse::<u16>().map_err(D::Error::custom),
        serde_json::Value::Number(n) => n
            .as_u64()
            .and_then(|x| u16::try_from(x).ok())
            .ok_or_else(|| D::Error::custom("port out of u16 range")),
        _ => Err(D::Error::custom("port must be a string or number")),
    }
}

impl RawConfig {
    fn resolve(self) -> Result<Config, String> {
        let world = parse_geometry(&self.world_geometry)
            .map_err(|e| format!("world_geometry {:?}: {e}", self.world_geometry))?;

        let mut cubes = Vec::with_capacity(self.cubes.len());
        let mut listeners = Vec::new();
        let mut offset = 0usize;

        for (ci, rc) in self.cubes.iter().enumerate() {
            let (width, height, length) = parse_geometry(&rc.dimensions)
                .map_err(|e| format!("cube {ci} dimensions {:?}: {e}", rc.dimensions))?;
            let cube = Cube { width, height, length, offset };

            for m in &rc.artnet_mappings {
                if m.port == 0 {
                    return Err(format!(
                        "cube {ci} mapping {}: port is required (loopback previz binds one \
                         UDP port per controller; see sim_config_*.json)",
                        m.ip
                    ));
                }
                if let Some(&z) = m.z_idx.iter().find(|&&z| z >= length) {
                    return Err(format!(
                        "cube {ci} mapping {}: z_idx {z} is outside cube length {length}",
                        m.ip
                    ));
                }
                listeners.push(Listener {
                    ip: m.ip.clone(),
                    port: m.port,
                    cube_index: ci,
                    z_indices: m.z_idx.clone(),
                });
            }

            offset += cube.voxel_count();
            cubes.push(cube);
        }

        if cubes.is_empty() {
            return Err("config has no cubes".to_string());
        }
        if listeners.is_empty() {
            return Err("config has no artnet_mappings with a port".to_string());
        }

        Ok(Config { cubes, listeners, num_voxels: offset, world })
    }
}

fn parse_geometry(s: &str) -> Result<(usize, usize, usize), String> {
    let parts: Vec<&str> = s.split('x').collect();
    if parts.len() != 3 {
        return Err("expected WxHxL".to_string());
    }
    let parse = |p: &str| p.parse::<usize>().map_err(|_| format!("bad integer {p:?}"));
    Ok((parse(parts[0])?, parse(parts[1])?, parse(parts[2])?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIM_CONFIG_4_CUBE0: &str = r#"{
      "world_geometry": "40x40x20",
      "cubes": [{
        "position": [0,0,0],
        "dimensions": "20x20x20",
        "artnet_mappings": [{"ip":"127.0.0.1","port":"6454","z_idx":[0,1,2,3,4]}]
      }]
    }"#;

    #[test]
    fn parses_string_port_and_geometry() {
        let cfg = Config::from_json_str(SIM_CONFIG_4_CUBE0).unwrap();
        assert_eq!(cfg.world, (40, 40, 20));
        assert_eq!(cfg.cubes.len(), 1);
        assert_eq!(cfg.cubes[0].voxel_count(), 20 * 20 * 20);
        assert_eq!(cfg.num_voxels, 8000);
        assert_eq!(cfg.listeners.len(), 1);
        assert_eq!(cfg.listeners[0].port, 6454);
        assert_eq!(cfg.listeners[0].cube_index, 0);
        assert_eq!(cfg.listeners[0].z_indices, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn napa_single_cube_is_56k_voxels() {
        let napa = r#"{
          "world_geometry": "20x20x140",
          "cubes": [{
            "position": [0,0,0],
            "dimensions": "20x20x140",
            "artnet_mappings": [{"ip":"127.0.0.1","port":51330,"z_idx":[0]}]
          }]
        }"#;
        let cfg = Config::from_json_str(napa).unwrap();
        assert_eq!(cfg.num_voxels, 56_000);
    }

    #[test]
    fn concatenates_multi_cube_offsets() {
        let two = r#"{
          "world_geometry": "40x20x20",
          "cubes": [
            {"position":[0,0,0],"dimensions":"20x20x20",
             "artnet_mappings":[{"ip":"127.0.0.1","port":6454,"z_idx":[0]}]},
            {"position":[20,0,0],"dimensions":"20x20x20",
             "artnet_mappings":[{"ip":"127.0.0.1","port":6455,"z_idx":[0]}]}
          ]
        }"#;
        let cfg = Config::from_json_str(two).unwrap();
        assert_eq!(cfg.cubes[0].offset, 0);
        assert_eq!(cfg.cubes[1].offset, 8000);
        assert_eq!(cfg.num_voxels, 16_000);
    }

    #[test]
    fn rejects_missing_port() {
        let bad = r#"{
          "world_geometry": "20x20x20",
          "cubes": [{"position":[0,0,0],"dimensions":"20x20x20",
            "artnet_mappings":[{"ip":"127.0.0.1","z_idx":[0]}]}]
        }"#;
        // serde will fail on the missing field before our port==0 guard.
        assert!(Config::from_json_str(bad).is_err());
    }

    #[test]
    fn rejects_z_idx_out_of_range() {
        let bad = r#"{
          "world_geometry": "20x20x20",
          "cubes": [{"position":[0,0,0],"dimensions":"20x20x20",
            "artnet_mappings":[{"ip":"127.0.0.1","port":6454,"z_idx":[20]}]}]
        }"#;
        assert!(Config::from_json_str(bad).is_err());
    }
}
