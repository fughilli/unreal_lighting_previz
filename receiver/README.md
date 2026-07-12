# previz volume receiver

A loopback **Art-Net sidecar** for the Napa previz (README §6.1). It binds one
UDP socket per virtual controller (from `artnet-tester`'s `config.json`),
reassembles the full voxel volume using the **same mapping as the hardware /
C++ simulator**, latches each frame on **ArtSync**, and publishes it to a POSIX
**shared-memory** region that the Unreal Engine component reads.

```
TouchDesigner / Python scenes / C++ sim
        │  Art-Net DMX (0x5000) + ArtSync (0x5200), loopback, 1 UDP port/controller
        ▼
  previz-receiver  ──(POSIX shm + seqlock, ~168 KB/frame)──▶  Unreal Engine
```

## Build & test (Bazel)

```sh
bazel test //receiver:previz_receiver_test     # unit tests (all modules)
bazel build //receiver:previz-receiver         # the sidecar binary
```

> First build fetches `rules_rust`, a Rust toolchain, and the `serde`/`libc`
> crates, so it needs network access.

## Run

```sh
# From previz/ — relative --config paths work (resolved via BUILD_WORKING_DIRECTORY):
bazel run //:receiver -- --config configs/dev_20x20x20.json --shm previz_dev
```

`//:receiver` is a root alias for `//receiver:previz-receiver`. Then drive it with
a known-good signal generator from the companion repo (a Python scene through the
sender), pointed at the same loopback ports — see `../ue/README.md` for the full
three-process live test.

Flags:

| flag | default | meaning |
|---|---|---|
| `--config <path>` | *(required)* | `artnet-tester` config JSON (must list `ip`+`port` per `artnet_mapping`) |
| `--shm <name>` | `previz_volume` | POSIX shm object name (a leading `/` is added) |
| `--universes-per-layer <n>` | `3` | must match the sender/sim (3 for a 20×20 cross-section) |
| `--quiet` | off | suppress per-listener / fps logging |

## Shared-memory contract (for the UE reader)

Region layout, little-endian, native alignment:

| offset | size | field |
|---|---|---|
| 0  | 4 | `magic` = `0x315A5650` ("PVZ1") |
| 4  | 4 | `version` = 1 |
| 8  | 4 | `num_voxels` |
| 12 | 4 | `width` |
| 16 | 4 | `height` |
| 20 | 4 | `length` |
| 24 | 4 | `cube_count` |
| 28 | 4 | reserved |
| 32 | 8 | `seq` (u64 seqlock; even = stable, odd = writer in progress) |
| 64 | `num_voxels*3` | RGB voxel data, `(z,y,x)` order, cubes concatenated |

**Reader protocol** (single-writer / single-reader seqlock):

```
loop:
  s1 = atomic_load(seq, acquire)
  if s1 is odd: continue            # writer mid-update
  copy DATA -> local buffer
  s2 = atomic_load(seq, acquire)
  if s1 == s2: done                 # consistent snapshot
```

Voxel `(x, y, z)` of cube *c* is at flat index
`cube[c].offset + z*W*H + y*W + x` (where `offset` is the running sum of prior
cubes' `W*H*L`). This matches the Python `Raster`'s `(length, height, width, 3)`
layout and the C++ simulator's pixel buffer, so the previz lights the exact
voxel the hardware would.

## What is ported vs. reused

- **Decode + mapping** (`artnet.rs`, `volume.rs`) is a direct port of
  `VolumetricDisplay::listenArtNet` in `artnet-tester` — that C++ simulator is
  the ground-truth oracle. (The repo's Rust `src/artnet` core is *send-only*, so
  the receive path is new code matching the C++ byte-for-byte.)
- **Config** (`config.rs`) consumes `artnet-tester`'s `config.json` unchanged.

## Status / limitations (v1 spike)

- Frame latch publishes the whole volume on **each** ArtSync. For the
  single-controller loopback path (one sync per frame) this is exactly frame
  coherent. Multi-controller configs sync independently — publishing on each
  sync is best-effort until a global all-controllers-synced latch is added.
- `layer_span = 1` (1:1 z mapping) is assumed, matching the locked install.
- No WS2812b color correction (linear emissive, README §6.2).
