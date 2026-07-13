# Cesium source patches (re-apply after any Cesium reinstall/update)

Cesium for Unreal is installed as source in `PrevizTest/Plugins/CesiumForUnreal`
and compiled locally. These local modifications must be re-applied if Cesium is
reinstalled:

1. **Load-radius excluder** — copy `CesiumLoadRadiusExcluder.{h,cpp}` into
   `CesiumForUnreal/Source/CesiumRuntime/{Public,Private}`. A
   `UCesiumTileExcluder` subclass with a `LoadRadius` (meters) property; add it as
   a component on a Cesium3DTileset so tiles beyond the radius are never fetched.

2. **Warnings-as-errors** (so Cesium's source compiles on UE 5.8's strict flags):
   in `CesiumRuntime.Build.cs` and `CesiumEditor.Build.cs`, after the `PCHUsage`
   line add:
       bWarningsAsErrors = false;
       CppCompileWarningSettings.UnreachableCodeWarningLevel = WarningLevel.Off;
       CppCompileWarningSettings.UninitializedWarningLevel = WarningLevel.Off;
   and wrap the self-capturing window decl in
   `CesiumEditor/Private/IonQuickAddPanel.cpp` (~line 141) with
   `#pragma clang diagnostic ignored "-Wuninitialized"` push/pop.

Caching is NOT a source patch — it's config in `DefaultEngine.ini`
(`[/Script/CesiumRuntime.CesiumRuntimeSettings] MaxCacheItems=200000`).

3. **Gaussian Splat crash guard** — `CesiumGaussianSplatSubsystem.cpp`, in
   `::Tick`, right before `getDataInterface()` is called (~line 268), add:
       if (!IsValid(this->_pNiagaraComponent)) {
         return;
       }
   Without it, `getDataInterface()` passes a null `_pNiagaraComponent` into
   `UNiagaraFunctionLibrary::GetDataInterface` and crashes on map open when the
   splat Niagara system fails to initialize (we don't use Gaussian splats).

4. **Session-stable request cache** — `CesiumRuntime.cpp`, in `getAssetAccessor()`.
   Google 3D Tiles put a per-run `session=` token in every tile URL, and
   CachingAssetAccessor keys by URL, so nothing hits across restarts. Two
   IAssetAccessor wrappers straddle the caching layer: `StripSessionAccessor`
   removes `?session=...&key=...` (stashing it in a header) so the cache key is
   session-independent; `RestoreSessionAccessor` re-appends it before the real
   fetch. Chain: Gunzip -> Strip -> Caching -> Restore -> Unreal. Full code in
   `session-stable-cache-CesiumRuntime.cpp.snippet`. (root.json has no `session=`
   so it's untouched -> each run still gets a fresh valid session for new tiles.)

~~5. **Two-sided tiles**~~ — **tried and reverted (2026-07-12): do NOT re-apply.**
   Setting **Two Sided = true** on `/CesiumForUnreal/Materials/M_CesiumBaseMaterial`
   was an attempt to fix uneven point-light terrain lighting (dark band above the
   light's z; hypothesis was inconsistently solved photogrammetry normals being
   hard-clamped by single-sided `max(0,N·L)` shading). It did not fix the issue,
   so the material is back to single-sided. The uneven-lighting cause is still
   unknown — reproduced with a plain hand-placed point light, so it is not the
   plugin's feed-driven light grid.
