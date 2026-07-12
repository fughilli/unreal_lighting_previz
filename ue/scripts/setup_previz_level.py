"""
Scripted setup of the NapaPreviz test in a UE5 project.

Creates an unlit emissive material (M_Voxel) that reads Per-Instance Custom Data
0/1/2 as RGB, spawns an Actor named "PrevizVolume" in the current level, adds a
UPrevizVolumeComponent, points it at the receiver's shared memory, and saves.

Run it either way:
  * In-editor:  Tools > Execute Python Script  (or the Output Log "Cmd" box: `py <path>`)
                Requires the "Python Editor Script Plugin" enabled.
  * Headless:   UnrealEditor-Cmd <Project>.uproject -ExecutePythonScript="<this file>"
                (quit the editor first so the project isn't locked).

Idempotent: re-running replaces M_Voxel and any prior "PrevizVolume" actor.

NOTE: The editor Python asset/graph API varies a little across UE versions; this
targets UE 5.x subsystem conventions. If a call name differs on 5.8, the error is
printed with a traceback — paste it and it's a quick fix.
"""

import traceback
import unreal

# ---- knobs -------------------------------------------------------------------
CONFIG = {
    "map_path": "/Game/PrevizMap",  # dedicated level (created if missing)
    "material_path": "/Game/M_Voxel",
    "strand_material_path": "/Game/M_Strand",
    "actor_label": "PrevizVolume",
    "shm_name": "previz_dev",     # must match previz-receiver --shm
    "voxel_spacing": 10.0,        # cm; real 10 cm LED pitch
    "voxel_scale": 0.01,          # 0.01 * 100 cm cube = 1 cm LEDs
    "emissive_gain": 12.0,        # bright emissive so the tiny LEDs bloom
    "strand_thickness": 1.0,      # cm; strand/diffuser rod cross-section
    "location": (300.0, 0.0, 100.0),  # 3 m in front, 1 m up — viewable from spawn
}


def log(msg):
    unreal.log("[previz-setup] " + msg)


def make_material():
    eal = unreal.EditorAssetLibrary
    mel = unreal.MaterialEditingLibrary
    path = CONFIG["material_path"]

    if eal.does_asset_exist(path):
        eal.delete_asset(path)

    tools = unreal.AssetToolsHelpers.get_asset_tools()
    mat = tools.create_asset("M_Voxel", "/Game", unreal.Material, unreal.MaterialFactoryNew())
    mat.set_editor_property("shading_model", unreal.MaterialShadingModel.MSM_UNLIT)
    # Required for the material to actually render on InstancedStaticMesh voxels;
    # without it UE silently falls back to the default (gray) material.
    mat.set_editor_property("used_with_instanced_static_meshes", True)

    # Build a float3 RGB from per-instance custom data indices 0,1,2.
    try:
        rgb = mel.create_material_expression(
            mat, unreal.MaterialExpressionPerInstanceCustomData3Vector, -500, 0)
        rgb.set_editor_property("data_index", 0)
    except AttributeError:
        # Older/variant API: three single-float nodes + AppendVector.
        r = mel.create_material_expression(mat, unreal.MaterialExpressionPerInstanceCustomData, -720, -120)
        r.set_editor_property("data_index", 0)
        g = mel.create_material_expression(mat, unreal.MaterialExpressionPerInstanceCustomData, -720, 0)
        g.set_editor_property("data_index", 1)
        b = mel.create_material_expression(mat, unreal.MaterialExpressionPerInstanceCustomData, -720, 120)
        b.set_editor_property("data_index", 2)
        a1 = mel.create_material_expression(mat, unreal.MaterialExpressionAppendVector, -520, -60)
        mel.connect_material_expressions(r, "", a1, "A")
        mel.connect_material_expressions(g, "", a1, "B")
        a2 = mel.create_material_expression(mat, unreal.MaterialExpressionAppendVector, -360, 0)
        mel.connect_material_expressions(a1, "", a2, "A")
        mel.connect_material_expressions(b, "", a2, "B")
        rgb = a2

    mul = mel.create_material_expression(mat, unreal.MaterialExpressionMultiply, -200, 0)
    mul.set_editor_property("const_b", CONFIG["emissive_gain"])
    mel.connect_material_expressions(rgb, "", mul, "A")
    mel.connect_material_property(mul, "", unreal.MaterialProperty.MP_EMISSIVE_COLOR)

    mel.recompile_material(mat)
    eal.save_asset(CONFIG["material_path"])
    log("created material " + CONFIG["material_path"])
    return mat


def make_strand_material():
    """Translucent, refractive 'glass rod' material for the vertical strands.
    Best-effort: some refraction properties vary by version, so they're guarded."""
    eal = unreal.EditorAssetLibrary
    mel = unreal.MaterialEditingLibrary
    path = CONFIG["strand_material_path"]
    if eal.does_asset_exist(path):
        eal.delete_asset(path)

    tools = unreal.AssetToolsHelpers.get_asset_tools()
    mat = tools.create_asset("M_Strand", "/Game", unreal.Material, unreal.MaterialFactoryNew())
    mat.set_editor_property("blend_mode", unreal.BlendMode.BLEND_TRANSLUCENT)
    mat.set_editor_property("shading_model", unreal.MaterialShadingModel.MSM_DEFAULT_LIT)
    mat.set_editor_property("used_with_instanced_static_meshes", True)
    try:
        mat.set_editor_property(
            "translucency_lighting_mode",
            unreal.TranslucencyLightingMode.TLM_SURFACE_PER_PIXEL_LIGHTING)
    except Exception as e:
        log("strand: translucency_lighting_mode skipped (%s)" % e)

    def constant(v, x, y):
        n = mel.create_material_expression(mat, unreal.MaterialExpressionConstant, x, y)
        n.set_editor_property("r", v)
        return n

    # Faint blue-white glass tint.
    tint = mel.create_material_expression(mat, unreal.MaterialExpressionConstant3Vector, -400, -200)
    tint.set_editor_property("constant", unreal.LinearColor(0.7, 0.85, 1.0, 1.0))
    mel.connect_material_property(tint, "", unreal.MaterialProperty.MP_BASE_COLOR)
    mel.connect_material_property(constant(1.0, -400, -60), "", unreal.MaterialProperty.MP_SPECULAR)
    mel.connect_material_property(constant(0.05, -400, 40), "", unreal.MaterialProperty.MP_ROUGHNESS)
    mel.connect_material_property(constant(0.15, -400, 140), "", unreal.MaterialProperty.MP_OPACITY)

    # Index-of-refraction refraction (~1.5, like acrylic/glass).
    try:
        mat.set_editor_property(
            "refraction_method", unreal.RefractionMode.REFRACTION_METHOD_INDEX_OF_REFRACTION)
        mel.connect_material_property(
            constant(1.5, -400, 240), "", unreal.MaterialProperty.MP_REFRACTION)
    except Exception as e:
        log("strand: refraction skipped (%s)" % e)

    mel.recompile_material(mat)
    eal.save_asset(path)
    log("created strand material " + path)
    return mat


def add_post_process():
    """Unbound PostProcessVolume: guarantee bloom is on and lock exposure so the
    LEDs don't get auto-dimmed. Best-effort; guarded per-setting."""
    actors = unreal.get_editor_subsystem(unreal.EditorActorSubsystem)
    for a in actors.get_all_level_actors():
        if a.get_actor_label() == "PrevizPostProcess":
            actors.destroy_actor(a)
    try:
        ppv = actors.spawn_actor_from_class(
            unreal.PostProcessVolume, unreal.Vector(0, 0, 0), unreal.Rotator(0, 0, 0))
        ppv.set_actor_label("PrevizPostProcess")
        ppv.set_editor_property("unbound", True)  # affect the whole scene
        s = ppv.get_editor_property("settings")
        # Bloom.
        s.set_editor_property("override_bloom_intensity", True)
        s.set_editor_property("bloom_intensity", 1.5)
        s.set_editor_property("override_bloom_threshold", True)
        s.set_editor_property("bloom_threshold", 0.3)
        # Adaptive eye-adaptation with a bounded range: lets the bright daytime
        # Cesium tiles AND a dark night scene with bright LEDs both resolve
        # (a fully-locked exposure blows out CesiumSunSky's physical sky to white).
        s.set_editor_property("override_auto_exposure_min_brightness", True)
        s.set_editor_property("auto_exposure_min_brightness", 0.03)
        s.set_editor_property("override_auto_exposure_max_brightness", True)
        s.set_editor_property("auto_exposure_max_brightness", 8.0)
        ppv.set_editor_property("settings", s)
        log("added PostProcessVolume (bloom on, manual exposure)")
    except Exception as e:
        log("post-process setup partial/failed (%s) — default bloom still applies" % e)


def place_actor(mat, strand_mat):
    eal = unreal.EditorAssetLibrary
    actors = unreal.get_editor_subsystem(unreal.EditorActorSubsystem)

    # Remove any prior run's actor.
    for a in actors.get_all_level_actors():
        if a.get_actor_label() == CONFIG["actor_label"]:
            actors.destroy_actor(a)

    loc = unreal.Vector(*CONFIG["location"])
    # APrevizVolumeActor owns a root + the component (constructed in C++), so we
    # don't need the editor's (5.8-removed) add_component_by_class.
    actor = actors.spawn_actor_from_class(unreal.PrevizVolumeActor, loc, unreal.Rotator(0, 0, 0))
    actor.set_actor_label(CONFIG["actor_label"])

    comp = actor.get_editor_property("volume")
    cube = eal.load_asset("/Engine/BasicShapes/Cube")
    comp.set_editor_property("shm_name", CONFIG["shm_name"])
    comp.set_editor_property("voxel_mesh", cube)
    comp.set_editor_property("voxel_material", mat)
    comp.set_editor_property("voxel_spacing", CONFIG["voxel_spacing"])
    comp.set_editor_property("voxel_scale", CONFIG["voxel_scale"])
    # Strands: reuse the engine cube (stretched to a rod by the component).
    comp.set_editor_property("strand_mesh", cube)
    if strand_mat:
        comp.set_editor_property("strand_material", strand_mat)
    comp.set_editor_property("strand_thickness", CONFIG["strand_thickness"])
    log("placed actor '%s' with PrevizVolumeComponent (shm=%s)" %
        (CONFIG["actor_label"], CONFIG["shm_name"]))
    return actor


def open_or_create_level():
    """Ensure a real, saveable level is loaded (headless has none by default)."""
    les = unreal.get_editor_subsystem(unreal.LevelEditorSubsystem)
    path = CONFIG["map_path"]
    if unreal.EditorAssetLibrary.does_asset_exist(path):
        les.load_level(path)
        log("opened existing level " + path)
    else:
        les.new_level(path)
        log("created level " + path)
    return les


def main():
    try:
        if not hasattr(unreal, "PrevizVolumeActor"):
            unreal.log_error(
                "[previz-setup] unreal.PrevizVolumeActor not found — is the "
                "NapaPreviz plugin enabled and compiled?")
            return
        les = open_or_create_level()
        mat = make_material()
        strand_mat = make_strand_material()
        place_actor(mat, strand_mat)
        add_post_process()
        les.save_current_level()
        unreal.EditorAssetLibrary.save_asset(CONFIG["map_path"])
        log("done — open %s, start previz-receiver, then press Play." % CONFIG["map_path"])
    except Exception:
        unreal.log_error("[previz-setup] FAILED:\n" + traceback.format_exc())


main()
