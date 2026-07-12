# Testing the previz in UE5

This is **Phase 1 step 3**: a UE5 plugin (`NapaPreviz`) that reads the volume
receiver's shared-memory buffer and lights emissive voxel instances from the
**live Art-Net feed**. The full live test is three processes on this one Mac:

```
  artnet-tester sender ──Art-Net+ArtSync (loopback :6454)──▶ previz-receiver
  (a scene, e.g. rainbow)                                    (publishes to shm)
                                                                   │
                                                            POSIX shm /previz_dev
                                                                   ▼
                                                     UE5 + NapaPreviz plugin
                                                     (emissive voxel instances)
```

The receiver + shm layout are done and tested. This plugin is the read side.

> ⚠️ The plugin C++ has **not** been compiled here (there's no UE on this
> checkout). Expect to fix the odd include/API detail for your exact UE version;
> the shared-memory logic mirrors `receiver/src/shmem.rs` exactly.

---

## 0. Prerequisites

- **UE5 installed** (5.3+ recommended).
- **Xcode + command-line tools** (`xcode-select --install`) — required to
  compile C++ for UE on macOS.
- The receiver built: from `previz/`, `bazel build //receiver:previz-receiver`.

## 1. Get a C++ project (you have a Blueprint-only project)

The plugin is C++, so the project must be able to compile C++. Either:

**A. Convert your existing BP project (easiest):** open it, then
`Tools ▸ New C++ Class… ▸ None ▸ Create Class`. UE adds a `Source/` folder and
turns it into a C++ project. Close the editor afterwards.

**B. New C++ project:** Epic Launcher / editor ▸ `Games ▸ Blank ▸ C++`.

## 2. Install the plugin

Copy (or symlink) this plugin into your project's `Plugins/` folder:

```sh
# from previz/
mkdir -p "/path/to/YourProject/Plugins"
cp -R ue/NapaPreviz "/path/to/YourProject/Plugins/NapaPreviz"
# (or: ln -s "$PWD/ue/NapaPreviz" "/path/to/YourProject/Plugins/NapaPreviz")
```

Regenerate project files and build:

- Right-click the `.uproject` ▸ **Generate Xcode project files** (or
  `Tools ▸ Refresh ... Project`), then build from Xcode, **or**
- just reopen the `.uproject` — the editor offers to rebuild missing modules; say yes.

Confirm the plugin is enabled: `Edit ▸ Plugins ▸ search "Napa Previz"`.

## 3. Make an emissive voxel material (one-time)

Content Browser ▸ **Add ▸ Material**, name it `M_Voxel`. Open it and:

1. Set **Shading Model = Unlit** (details panel).
2. Add three **`PerInstanceCustomData`** nodes with indices **0, 1, 2**
   (search "PerInstanceCustomData" in the palette). These are R, G, B.
3. `AppendVector`/`MakeFloat3` them into a float3, optionally `Multiply` by a
   scalar (e.g. **5.0**) so bloom kicks in.
4. Plug into **Emissive Color**. Save.

A basic **cube** mesh comes with the engine (`Engine ▸ BasicShapes ▸ Cube`), or
make a tiny one. You'll assign both on the component next.

## 4. Place the volume in a level

1. Drag any **Actor** into the level (an empty Actor is fine). Reset its
   transform to the origin.
2. With it selected: `Add Component ▸ Previz Volume` (the
   `UPrevizVolumeComponent`).
3. In the component's **Previz** category set:
   - **Shm Name** = `previz_dev` (must match the receiver's `--shm`).
   - **Voxel Mesh** = the Cube.
   - **Voxel Material** = `M_Voxel`.
   - **Voxel Spacing** = `15` (cm; real pitch) — or larger to spread them out.

Even with no mesh/material, the component logs `frames=… center voxel=(r,g,b)`
so you can confirm data is arriving.

## 5. Run the live test (three terminals)

**Terminal 1 — receiver** (from `previz/`):

```sh
bazel run //:receiver -- --config configs/dev_20x20x20.json --shm previz_dev
```

**Terminal 2 — signal source** (from `~/Projects/artnet-tester`):

```sh
bazelisk run //:sender -- \
  --config "$HOME/Projects/wakenmake/projects/napa_lighted_art_festival/previz/configs/dev_20x20x20.json" \
  --scene  "$HOME/Projects/artnet-tester/rainbow_scene.py"
```

✅ **Checkpoint (no UE needed):** Terminal 1 should start printing
`published N frames (… fps)`. That alone proves sender → receiver → shm works.

**Terminal 3 — UE:** press **Play**. The voxel grid should animate with the
rainbow scene. Watch **Output Log** (filter `LogNapaPreviz`) for the connect
line and per-second stats.

For the real geometry, swap both `--config` to `configs/napa_20x20x140.json` and
set the component's **Shm Name** to `previz_napa` (56,000 voxels).

> `bazel run //:receiver` and the raw `bazel-bin/receiver/previz-receiver` binary
> behave identically — relative `--config` paths resolve from the directory you
> ran the command in either way.

---

## Troubleshooting

- **`shm_open(...) failed … is previz-receiver running?`** — start the receiver
  (Terminal 1) before/again; ShmName must match `--shm` (no leading `/` needed).
- **Receiver prints 0 fps** — the sender isn't reaching it. Confirm both use the
  **same config** (same port 6454) and the sender scene loaded without error.
- **Voxels are black but stats show nonzero RGB** — material isn't reading
  PerInstanceCustomData; re-check the three indices and the Emissive wiring.
- **Everything black + center voxel (0,0,0)** — the scene may light only some
  voxels; try `sphere_scene.py` or `full_white_scene.py`.
- **Multi-cube configs (`sim_config_4/8`) look scrambled** — those concatenate
  per-cube buffers; use a **single-cube** config (the ones in `configs/`) for the
  grid path. Multi-cube placement needs per-cube offsets (future work).
- **Perf** — 56k instances updating custom data each tick is fine on M-series,
  but if it chugs, test with `dev_20x20x20` (8k) first or raise VoxelSpacing.

## What this proves / what's next

Running this closes **Phase 1**: Cesium streaming (step 1) + live-Art-Net into a
real-time render (steps 2–3). Known gaps after: georeferenced placement inside
Cesium (Phase 3), multi-cube offsets, WS2812b color match, and the raymarch
look alternative.
