# Light Art Previz

A previsualization environment for site-specific light-art installations. It pulls
real-world 3D site geometry, places a virtual model of the light sculpture into that
context, and renders the result in real time while being driven by the **live Art-Net
feed** from the same controller that will run the physical piece.

**First target:** the hanging volumetric LED installation for the Napa Lighted Art
Festival — a voxel volume in the public plaza near the Historic Napa County Courthouse,
rendered to video for design iteration and stakeholder review.

> Status: proposal / request-for-comment. Nothing is built yet. Open questions are
> flagged inline as **[OPEN]**.

---

## 0. Handoff state (read this first)

**As of 2026-07-02.** This document is an approved proposal. The architecture and v1
decisions below are settled with the project owner. **Phase 1 step 2 (the volume
receiver) is implemented, built, and its unit tests pass**; UE steps 1 and 3 still need
the editor and are untouched (see "Immediate next step" below).

**Build status — verified green, incl. a live loopback run.**
`bazel test //receiver:previz_receiver_test` → **16/16 pass**;
`bazel build //receiver:previz-receiver` succeeds. **Live path verified end-to-end:** a
faithful sender (exact artnet-tester packetization) over real UDP → receiver decode →
ArtSync latch → shm publish → seqlock read back returned the correct voxels (sampled 4/4,
seq counter correct) using `configs/dev_20x20x20.json`. *What's still unproven:* (a) the
cross-check against the **actual** artnet-tester bazel sender / C++ `VolumetricDisplay`
specifically (its bazel build was too slow to run headlessly here), and (b) the **UE
reader** — steps 1 & 3. The UE plugin is written (`ue/NapaPreviz/`) but not yet compiled.

**What's in `previz/` now:**
- `MODULE.bazel`, `.bazelrc`, root + `receiver/BUILD.bazel`, `Cargo.toml`/`Cargo.lock` —
  self-contained Bazel module (`napa-previz`), mirroring artnet-tester (Bazel 7.4.0,
  `rules_rust` 0.61.0). **Everything in this project is Bazel-managed.**
- `receiver/src/` — `config.rs` (consumes artnet-tester `config.json`), `artnet.rs`
  (ArtDmx/ArtSync decode), `volume.rs` (universe→`(z,y,x)` reassembly), `shmem.rs`
  (POSIX shm + seqlock), `main.rs` (one UDP socket per controller, latch on ArtSync,
  publish to shm). See `receiver/README.md` for the shm contract + run instructions.
- **Heads-up for the next agent:** the repo's Rust `src/artnet` core is **send-only** —
  there is no Rust receiver to "reuse". The receive path was instead ported byte-for-byte
  from the C++ `VolumetricDisplay::listenArtNet`, which is the ground-truth oracle.

**Locked decisions (don't relitigate without asking the owner):**
- Engine: **Unreal Engine 5** + **Cesium for Unreal** (Google Photorealistic 3D Tiles).
- Render/playback host: **one Apple Silicon MacBook** (this machine). Path tracer is unavailable on Mac → use real-time **Lumen**.
- Ingest: **custom sidecar receiver** that reuses `~/Projects/artnet-tester`'s Rust Art-Net core and its `config.json` mapping — **not** UE's DMX plugin.
- Networking: **pure loopback**, one UDP port per virtual controller (scenes + previz on the same machine).
- Receiver → UE transport: **POSIX shared memory (`mmap`)**, ~168 KB/frame.
- Look: discrete emissive points + bloom + volumetric fog; **linear emissive** (no WS2812b color match in v1).

**The authoritative companion repo is `~/Projects/artnet-tester`** — it is the lighting
install's real control software and the source of truth for geometry + protocol. Before
writing the receiver, read:
- `config.json` — geometry (`20x20x20` here; the Napa volume is `20x20x140`), orientation, per-controller IP→z-layer mapping. **The previz must consume this file, not reinvent it.**
- `artnet.py` — the `Raster` data model `(z,y,x,rgb)` and the exact Art-Net packing (510 ch/universe, opcode `0x5000`, ArtSync `0x5200`).
- `src/artnet/core.rs` — the Rust Art-Net packet format/constants. **Note: send-only** (no receiver), so it's a format reference, not a receive path to reuse.
- `VolumetricDisplay.cpp` / `.h` — a working C++ OpenGL voxel renderer that already ingests this Art-Net; the `listenArtNet` method is **the ground-truth oracle the receiver's decode/mapping was ported from**, and the reference for live test cross-checks.
- `gen_routing_table.py` — universe-numbering scheme (`universe_base = layer*universes_per_layer + i`).
- Python scenes (`rainbow_scene.py`, `sphere_scene.py`, `plane_scene.py`) + `touchdesigner/donut.toe` / `volumetric.tox` — known-good signal generators for testing.

**Open questions that do NOT block Phase 1** (the spike can proceed with placeholders;
confirm with owner before Phase 3): altitude datum + heading of the hang anchor;
discrete-points vs. raymarched look; Google 3D Tiles licensing for video; plaza
photogrammetry feasibility; offline hero pass / RTX box; deliverable spec; TD frame rate.
See §11.

**Immediate next step — Phase 1 spike (de-risk the two hard integrations):**
1. ⬜ Create a UE5 project on the Mac; install **Cesium for Unreal**; set a `CesiumGeoreference` to **38.297189, -122.284700** and stream Google Photorealistic 3D Tiles of the courthouse block. *Acceptance: the Napa courthouse + surrounding buildings render in-editor.* **(Needs the UE editor + a Google Maps Platform API key — owner/human task.)**
2. ✅ *(built + unit-tested; live cross-check still pending)* **Volume receiver** in `receiver/`: binds loopback ports per `config.json`, decodes + latches on ArtSync into the RGB voxel buffer, writes it to a POSIX shared-memory region (seqlock; layout in `receiver/README.md`). Verified: `bazel test //receiver:previz_receiver_test` (16/16), binary builds and starts against `sim_config_4.json`. **Remaining acceptance:** drive it with a known scene from `artnet-tester` and confirm the shm buffer matches what the C++ simulator shows for the same feed.
3. 🟡 *(plugin written, not yet compiled)* In UE, a C++ component `mmap`s the buffer and lights emissive instances from it. Delivered as the **`NapaPreviz` UE plugin** (`ue/NapaPreviz/`) with a `UPrevizVolumeComponent` (seqlock reader + InstancedStaticMesh voxels). **Full step-by-step to run the live test is in `ue/README.md`.** *Acceptance: changing the scene in `artnet-tester` visibly changes the emissive geometry live in UE.* **(Needs a UE C++ project; the plugin C++ hasn't been compiled here.)**

Doing 1–3 proves Cesium streaming and live-Art-Net-into-real-time-render end to end.
Everything after is in §10.

---

## 1. The specific install

Pulled from the existing control software in `~/Projects/artnet-tester` (the
authoritative source for geometry and protocol):

| Property | Value |
|---|---|
| Voxel volume | **20 × 20 × 140** = **56,000 voxels** |
| Physical size | 3 m × 3 m × 21 m (uniform **15 cm** voxel pitch) |
| Form | Hanging LED curtains (vertical sheets), suspended **3 m** above grade → spans ~3–24 m elevation |
| Content | A **3D fluid simulation** authored and played from **TouchDesigner** |
| Control protocol | **Art-Net** DMX (opcode `0x5000`) + **ArtSync** (`0x5200`) frame latch |
| Data model | `Raster` in `(z, y, x, rgb)` order, uint8; one z-layer sent per chunk, row-major `(y,x)` |
| Packing | 510 channels / universe (170 RGB pixels); **~330+ universes**, **~168k channels** total |
| LED type | WS2812b (hardware applies a `ReverseColorCorrector`) |
| Look target | **Emissive + bloom + volumetric fog** — no per-pixel scene light-casting needed for v1 |

The crucial consequence: **the previz never needs its own pixel-map format.** It reads
`artnet-tester`'s `config.json` (geometry, orientation, per-controller IP → z-layer
mapping) so the previz and the real hardware are guaranteed to interpret the same
Art-Net identically.

---

## 2. Goals & constraints

**Must have**
- Import detailed 3D geometry of the actual site (buildings, ground, courthouse) around the plaza.
- Place the 56k-voxel volume, correctly scaled and georeferenced, hanging in the plaza.
- Be driven by the **live Art-Net feed** from TouchDesigner — no separate bake/export step.
- Render night-time conditions and produce **video** output.
- Real-time, so the artist can change the fluid sim and immediately see it in context.

**Platform:** render + scene playback run on the **same macOS / Apple Silicon MacBook** (loopback networking; no second box in v1). This has one consequence worth stating up front: **UE's path tracer is unavailable on Mac** (it's DXR/NVIDIA-only). Real-time **Lumen** GI/reflections *do* run on Apple Silicon, so the beauty pass is fine — but a path-traced offline "hero" render would need a separate RTX/Windows box (see §7).

**Nice to have:** offline high-fidelity "hero" pass; reusable across future sites/sculptures; sACN support.

**Non-goals (v1):** photometrically calibrated luminance prediction; driving physical hardware (we only *consume* control data); per-pixel illumination of the environment.

---

## 3. Recommended stack at a glance

| Concern | Choice | Build vs. buy |
|---|---|---|
| Real-time engine + renderer + video out | **Unreal Engine 5** | COTS |
| Site geometry (buildings/terrain) | **Cesium for Unreal** streaming **Google Photorealistic 3D Tiles** | COTS |
| Art-Net/ArtSync ingest + volume reassembly | **Custom receiver reusing `artnet-tester`'s Rust Art-Net core + `config.json`** | **Custom (high reuse)** |
| Receiver → UE transport (macOS) | **POSIX shared memory (`mmap`) + CPU texture upload** | **Custom** |
| 56k-voxel → engine data path | **DMX volume → 3D/atlas texture → emissive instances** | **Custom** |
| Georeferenced placement | **Cesium globe anchor + thin placement tool** | **Custom (thin)** |
| Capture / recording | **Movie Render Graph + live capture** | COTS + glue |
| Test signal generation | **Reuse existing Python scenes, C++ simulator, TD `.tox`** | **Reuse** |

**Why Unreal over Blender:** the decisive requirement is *live control data into a
real-time, photoreal render*. UE5 gives us **Cesium for Unreal** (the clean, licensed
route to the Google Earth city mesh as OGC 3D Tiles) plus a mature path-traced/Lumen
renderer and **Movie Render Graph** for video — the two hard problems (site geometry,
beauty rendering) are solved by maintained COTS. Blender (EEVEE) renders in real time
but has neither first-party 3D Tiles streaming nor a clean live-Art-Net path; we'd
build both. Blender stays in the pipeline for **asset prep** (mesh cleanup, the
sculpture's CAD/curtain model, georeferencing photogrammetry). **[OPEN]** Revisit if the
team's UE experience is shallow.

**Why a custom receiver instead of UE's built-in DMX plugin:** at ~330+ universes with
**ArtSync** frame coherency and a bespoke per-controller-IP → z-layer mapping, the
first-party DMX plugin is an awkward fit (per-universe object overhead; ArtSync latch
support is uncertain). Instead we reuse the already-working, already-tested Art-Net
logic from `artnet-tester` (the same code the C++ simulator and Rust sender use) as a
thin sidecar that reassembles the full volume and streams it to UE as a texture. This
maximizes reuse and isolates the networking firehose from the engine.

---

## 4. Architecture

```
  TouchDesigner ──Art-Net DMX (0x5000) + ArtSync (0x5200)──┐
  (3D fluid sim)   ~330 universes, ~168k channels           │
                   loopback (127.0.0.1), 1 UDP port/controller
                                                           ▼
                              ┌──────────────────────────────────────────┐
                              │  Volume receiver  (CUSTOM, reuses          │
                              │  artnet-tester Rust artnet core + config)  │
                              │  • bind 127.0.0.1, one port per virtual    │
                              │    controller                              │
                              │  • map (universe,channel) → (z,y,x) via     │
                              │    config.json  (single source of truth)   │
                              │  • latch full frame on ArtSync             │
                              │  • write 56k RGB into a volume buffer      │
                              └───────────────┬────────────────────────────┘
                                              │ POSIX shared memory (mmap), ~168 KB/frame
                                              ▼
  ┌───────────────────────────── Unreal Engine 5 ─────────────────────────────┐
  │                                                                            │
  │  DMX volume texture ──▶ Sculpture actor                Site context        │
  │  (3D tex or 2D atlas)   • 56k emissive instances       Cesium for Unreal   │
  │                           sample color by index        → Google Photoreal  │
  │                         • bloom + local volumetric       3D Tiles          │
  │                           fog (sells the "volume")     (buildings/terrain) │
  │                         • GlobeAnchor placement                            │
  │                                                                            │
  │  Cine camera + night lighting/exposure ──▶ live viewport / SDI / NDI       │
  │                                          └▶ Movie Render Graph (record)    │
  └────────────────────────────────────────────────────────────────────────────┘

  Test harness (CUSTOM + REUSE): existing Python scenes / C++ sim / TD .tox
  emit known Art-Net → assert correct voxel lit/colored/placed in-engine.
```

### Data flow
1. TouchDesigner emits Art-Net + ArtSync on the LAN.
2. The **volume receiver** reassembles all universes into a single 56k-voxel RGB buffer, using `config.json` for the channel→voxel mapping, latching each frame on ArtSync.
3. It streams the buffer to UE as a **DMX volume texture** (3D texture, or a 2D atlas of z-slices).
4. The **sculpture actor** renders 56k emissive elements that sample their color from that texture by index; **bloom + volumetric fog** make the discrete LEDs read as a glowing volume at night (faithful to how 15 cm-pitch curtains actually look from plaza distance).
5. A **Cesium globe anchor** positions the volume at the real hang coordinates inside the streamed building geometry.
6. A cine camera frames the shot; output is captured live and/or recorded with Movie Render Graph.

---

## 5. Site geometry: getting the buildings in

Best → most caveated:

1. **Google Photorealistic 3D Tiles via Cesium for Unreal (recommended).** Google
   publishes the city mesh through the Map Tiles API as OGC **3D Tiles** (glTF-based);
   Cesium streams them live, georeferenced. This is the supported, license-clean route
   to "the Google Earth geometry." Needs a Google Maps Platform API key. **[OPEN]**
   confirm licensing permits rendered-video output + attribution requirements.
2. **Local photogrammetry of the plaza (recommended in addition).** Google's mesh is
   great for the skyline but mushy on the exact courthouse façade and plaza trees where
   the piece hangs. A short drone/ground capture (RealityCapture/Metashape) gives a
   crisp near-field to drop on top, georeferenced. **[OPEN]** site access / drone
   permitting feasible before the deadline?
3. **OSM footprints + extrusion (fallback / massing).** Blocky context for occlusion
   and shadows if the Tiles route is blocked.

Coordinate handling: Cesium's `CesiumGeoreference` converts geographic (ECEF) ↔ UE's
cm / Z-up space; the volume's hang point is authored as real lat/long/alt so it's
reproducible and survives an origin rebase.

---

## 6. The sculpture in-engine (custom work, concentrated)

### 6.1 Volume receiver (reuses existing code)
A sidecar that reuses `artnet-tester`'s Rust Art-Net implementation (`src/artnet`) and
ingests its `config.json`. Because scene playback and previz run on the **same Mac**,
networking is pure **loopback**: each virtual controller sends to `127.0.0.1` on its own
UDP port, and the receiver binds one socket per controller — no multi-IP binding, no
network tap. Responsibilities: decode DMX, apply the `(z,y,x)` mapping, latch on
ArtSync, and publish the full 56k-voxel RGB volume.

Transport to UE: **POSIX shared memory (`mmap`)** — the receiver writes the latched
volume (56k × 3 B ≈ **168 KB/frame**, ~10 MB/s at 60 fps; trivial) into a shared buffer;
a custom UE C++ component `mmap`s it and uploads to a texture each frame via
`UpdateTextureRegions`. This is Mac-native, lowest-latency, and fully under our control —
**Spout is Windows-only**, and NDI/Syphon add video-codec/latency overhead we don't need
for a 168 KB buffer. A single-writer/single-reader double-buffer (or seqlock) avoids tearing.

### 6.2 Emissive volume rendering
56k instances is comfortable for UE. Each LED is an emissive element sampling its color
from the DMX volume texture by index; **bloom + local volumetric fog** provide the
glow/scatter. We render **discrete points** rather than a raymarched continuum because
that's faithful to the physical curtains; a volume-texture raymarch is available as an
alternate aesthetic if the fluid sim reads better as a continuum. **[OPEN]** confirm
discrete-points look is desired.

Color fidelity: **v1 uses linear/sRGB emissive** — no WS2812b response-curve matching.
(Mirroring `color_correction.h` is deferred to a later pass if color-accurate previz is
ever needed.)

### 6.3 Placement
Hang anchor is set: **38.297189, -122.284700** (plaza by the Historic Napa County
Courthouse). A thin tool sets the `GlobeAnchor` to that lat/long plus **[OPEN]** the
**altitude** of the hang point (top of the 21 m volume, or its base 3 m above grade —
need to fix the datum) and the **heading** of the 20×20 cross-section (which horizontal
axis faces which compass bearing — drives how the curtains present to viewers), and
aligns local photogrammetry to the Cesium context.

---

## 7. Capture & output

- **Live / real-time (primary):** engine runs live against the Art-Net feed with **Lumen** for GI; capture via Movie Render Graph's real-time path or a screen/window recorder. Satisfies "no bake / no sub-realtime playback" and runs on the Mac.
- **Offline hero pass (optional, not on this Mac):** UE's path tracer is **NVIDIA/DXR-only**, so a path-traced beauty render isn't possible on Apple Silicon. If we want one, the options are (a) push real-time Lumen quality (often enough at night) and capture at high res, or (b) move the offline pass to a Windows/RTX box. **[OPEN]** needed for v1? If yes, is a Windows/RTX box available?

---

## 8. What we write vs. get for free

**Off-the-shelf:** UE5, Cesium for Unreal + Google 3D Tiles, Movie Render Graph,
RealityCapture/Metashape (if doing photogrammetry), Blender (asset prep).

**Reused from `artnet-tester`:** Art-Net Rust core (`src/artnet`), `config.json`
(geometry + mapping), the C++ OpenGL voxel simulator (reference + fallback renderer),
Python scenes (`rainbow_scene.py`, `sphere_scene.py`, `plane_scene.py`, …) and the
TouchDesigner `volumetric.tox`/`donut.toe` as test/content sources, `controller_simulator.py`.

**Custom (the actual engineering):**
1. Volume receiver (sidecar) + UE transport.
2. DMX-volume-texture path + emissive sculpture actor/material.
3. Thin georeferenced placement tool.
4. Capture/recording glue (timecode, start/stop, optional Art-Net record).
5. Test harness (§9).

The custom surface is deliberately small and concentrated on the *bespoke* glue;
everything generic is COTS or reused.

---

## 9. Testing strategy

Goal: trust that "the voxel TouchDesigner addressed is the voxel that lit, in the right
color and the right place" — without eyeballing every frame. The big advantage here is
that the existing repo already gives us **known-good signal generators**.

- **Unit**
  - Art-Net/ArtSync decode against spec vectors (universe little-endian, opcodes, sync latch).
  - `config.json` → voxel mapping: round-trip a known `(z,y,x)` ↔ (universe, channel) and assert it matches `gen_routing_table.py` / the C++ sim exactly.
  - Coordinate conversions (voxel → local → georeferenced → UE) against known reference points.
- **In-engine loopback (UE automation / Gauntlet)**
  - Replay a known scene (e.g. `plane_scene.py` lights one z-layer; a single-voxel scene; a per-axis ramp) through the receiver, sample UE's render target, assert the expected voxels lit with expected color. Cross-check against the C++ simulator rendering the *same* Art-Net as ground truth.
- **Visual / golden-frame regression**
  - Fixed camera + fixed Art-Net frame → render → compare to approved golden within tolerance. Catches drift in fog/bloom/exposure/materials.
- **Geospatial accuracy**
  - Volume anchor lands at surveyed hang coordinates; courthouse corners align with reference photos from matched camera positions.
- **Performance / latency**
  - Sustain target fps at 56k voxels / ~330 universes; measure Art-Net-in → frame-out latency (must feel live at the TD console).
- **End-to-end**
  - Drive from the real TouchDesigner fluid-sim project and record a clip — the real acceptance test.

---

## 10. Proposed phasing

1. **Spike / de-risk (1–2 wk):** UE5 + Cesium streaming the Napa courthouse block; volume receiver reassembling one frame from the C++ sim's/Python sender's Art-Net and lighting a *hardcoded* emissive slab. Proves the two riskiest integrations end-to-end.
2. **Sculpture pipeline:** receiver → DMX volume texture → 56k emissive instances at full count; night look (bloom/fog/exposure); WS2812b color match if in scope.
3. **Site integration:** georeferenced placement; optional plaza photogrammetry merged with Cesium.
4. **Capture:** live recording path; optional offline path-traced pass.
5. **Test harness + hardening:** loopback automation vs. C++ sim ground truth, golden frames, perf/latency budget, full E2E from TouchDesigner.

---

## 11. Open questions (consolidated)

Resolved: network = loopback, one UDP port per virtual controller (§6.1); transport =
POSIX shared memory (§6.1); color = linear emissive for v1 (§6.2); hang lat/long =
38.297189, -122.284700 (§6.3).

Still open:
- **Altitude & heading** of the hang point — datum for the 21 m volume and the compass bearing of the 20×20 cross-section. (§6.3)
- **Look:** discrete emissive points (faithful) vs. raymarched volume. (§6.2)
- **Geometry source:** Google 3D Tiles licensing for rendered video; feasibility of a plaza photogrammetry capture. (§5)
- **Offline hero pass:** needed for v1? If yes, is a Windows/RTX box available (path tracer can't run on the Mac)? (§7)
- **Deliverable spec & deadline:** length, resolution, number of shots; engine choice confirmation given team skills. (§3)
- **Frame rate** of the TD fluid sim (drives the latency/perf budget). (§9)
