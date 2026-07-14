"""
Add the site geometry (Google Photorealistic 3D Tiles via Cesium) to the
previz level, georeferenced at the hang anchor, and move the LED volume there.

Run headless:
  UnrealEditor-Cmd <Project>.uproject -ExecutePythonScript="<this file>"
(Cesium must be installed + its dylibs rpath-patched — see fix_cesium_rpaths.sh.)

The Google Maps Platform API key is read from a file so it never appears in this
script or any log. It does get baked into the saved tileset URL in the .umap
(same as configuring it through the Cesium UI).

Idempotent: re-running reuses actors by label.
"""

import os
import traceback

import unreal

# ---- knobs -------------------------------------------------------------------
HANG_LAT = 38.297189
HANG_LON = -122.284700
# Approx WGS84 *ellipsoidal* ground height at the default site plaza (orthometric ~6 m
# minus geoid ~ -32 m). This is a starting estimate — fine-tune visually so the
# volume sits at plaza level (the altitude datum is an open question, README).
ORIGIN_HEIGHT = -26.0
# Volume transform, tuned in-editor against the streamed tiles (2026-07-13).
# The runner's module-stack axis is the volume's local Z; the 90 deg roll lays
# it horizontally down the corridor and the yaw aligns it. Rotation tuple is
# (roll, pitch, yaw) = the editor's X, Y, Z rotation fields.
VOLUME_POS = (-200.0, 0.0, 400.0)  # cm, relative to the georeferenced origin
VOLUME_ROT = (90.0, 0.0, -33.0)
MAP_PATH = "/Game/PrevizMap"
KEY_FILE = os.path.expanduser("~/Projects/scratch/credentials/maps_api_key.txt")
GOOGLE_TILES_URL = "https://tile.googleapis.com/v1/3dtiles/root.json?key=%s"


RESULT_FILE = "/tmp/cesium_setup_result.txt"


def log(msg):
    unreal.log("[cesium-setup] " + msg)
    with open(RESULT_FILE, "a") as f:
        f.write(msg + "\n")


def actors():
    return unreal.get_editor_subsystem(unreal.EditorActorSubsystem)


def get_or_spawn(cls, label):
    for a in actors().get_all_level_actors():
        if a.get_actor_label() == label:
            return a
    a = actors().spawn_actor_from_class(cls, unreal.Vector(0, 0, 0), unreal.Rotator(0, 0, 0))
    a.set_actor_label(label)
    return a


def main():
    try:
        les = unreal.get_editor_subsystem(unreal.LevelEditorSubsystem)
        if unreal.EditorAssetLibrary.does_asset_exist(MAP_PATH):
            les.load_level(MAP_PATH)

        with open(KEY_FILE) as f:
            key = f.read().strip()

        # Clean slate: remove any Cesium actors from prior runs so we don't
        # accumulate duplicate georeferences / camera managers / credit systems.
        for a in list(actors().get_all_level_actors()):
            if a.get_class().get_name().startswith("Cesium"):
                actors().destroy_actor(a)

        # 1) Google Photorealistic 3D Tiles. Spawning it auto-creates the single
        #    "default" georeference that everything else (sun/sky, anchors) shares.
        tileset = actors().spawn_actor_from_class(
            unreal.Cesium3DTileset, unreal.Vector(0, 0, 0), unreal.Rotator(0, 0, 0))
        tileset.set_actor_label("GooglePhotorealistic3DTiles")
        tileset.set_editor_property("tileset_source", unreal.TilesetSource.FROM_URL)
        tileset.set_editor_property("url", GOOGLE_TILES_URL % key)
        tileset.set_editor_property("show_credits_on_screen", True)  # Google attribution (required)
        log("Google 3D Tiles tileset configured")

        # 2) Configure the resolved (default) georeference for the hang anchor.
        georef = tileset.resolve_georeference()
        georef.set_actor_label("CesiumGeoreference")
        georef.set_editor_property("origin_placement", unreal.OriginPlacement.CARTOGRAPHIC_ORIGIN)
        # Takes a single FVector: x=longitude, y=latitude, z=height.
        georef.set_origin_longitude_latitude_height(
            unreal.Vector(HANG_LON, HANG_LAT, ORIGIN_HEIGHT))
        log("georeference @ %.6f, %.6f, h=%.1f" % (HANG_LAT, HANG_LON, ORIGIN_HEIGHT))

        # 3) Sun/sky so the (lit) tiles are visible; it resolves to the same
        #    default georeference. Defaults give daylight (night-grade later).
        try:
            actors().spawn_actor_from_class(
                unreal.CesiumSunSky, unreal.Vector(0, 0, 0), unreal.Rotator(0, 0, 0)
            ).set_actor_label("CesiumSunSky")
            log("CesiumSunSky added")
        except Exception as e:
            log("CesiumSunSky failed (%s) — add a DirectionalLight manually" % e)

        # 4) Only one directional light can drive forward shading (translucency,
        #    volumetrics); with both CesiumSunSky's sun and the NightMoonlight
        #    present UE warns and picks arbitrarily. Make the moonlight (the
        #    night key light) win deterministically.
        for a in actors().get_all_level_actors():
            comp = a.get_component_by_class(unreal.DirectionalLightComponent)
            if comp:
                pri = 1 if a.get_actor_label() == "NightMoonlight" else 0
                comp.set_editor_property("forward_shading_priority", pri)
                log("forward_shading_priority=%d on %s" % (pri, a.get_actor_label()))

        # 5) Move the LED volume to the georeferenced hang point.
        for a in actors().get_all_level_actors():
            if a.get_actor_label() == "PrevizVolume":
                a.set_actor_location(unreal.Vector(*VOLUME_POS), False, False)
                a.set_actor_rotation(
                    unreal.Rotator(
                        roll=VOLUME_ROT[0], pitch=VOLUME_ROT[1], yaw=VOLUME_ROT[2]),
                    False)
                log("moved PrevizVolume to %s, rot (roll,pitch,yaw)=%s"
                    % (VOLUME_POS, VOLUME_ROT))

        les.save_current_level()
        unreal.EditorAssetLibrary.save_asset(MAP_PATH)
        log("done — open %s and Play; the courthouse block should stream in." % MAP_PATH)
    except Exception:
        tb = traceback.format_exc()
        unreal.log_error("[cesium-setup] FAILED:\n" + tb)
        with open(RESULT_FILE, "a") as f:
            f.write("FAILED:\n" + tb)


open(RESULT_FILE, "w").close()  # truncate
main()
