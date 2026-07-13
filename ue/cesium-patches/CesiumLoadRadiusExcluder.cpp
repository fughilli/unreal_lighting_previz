// Previz addition: distance-based tile excluder.

#include "CesiumLoadRadiusExcluder.h"
#include "CesiumTile.h"
#include "Camera/PlayerCameraManager.h"
#include "Engine/World.h"
#include "GameFramework/PlayerController.h"

#if WITH_EDITOR
#include "Editor.h"
#include "LevelEditorViewport.h"
#endif

namespace {
// Best-effort current view location (Unreal world space): the perspective
// editor viewport when not playing, otherwise the player camera.
bool GetViewLocation(UWorld* World, FVector& OutLocation) {
#if WITH_EDITOR
  if (GEditor && (!World || !World->IsGameWorld())) {
    for (FLevelEditorViewportClient* ViewportClient :
         GEditor->GetLevelViewportClients()) {
      if (ViewportClient && ViewportClient->IsPerspective()) {
        OutLocation = ViewportClient->GetViewLocation();
        return true;
      }
    }
  }
#endif
  if (World) {
    if (APlayerController* PC = World->GetFirstPlayerController()) {
      if (PC->PlayerCameraManager) {
        OutLocation = PC->PlayerCameraManager->GetCameraLocation();
        return true;
      }
    }
  }
  return false;
}
} // namespace

bool UCesiumLoadRadiusExcluder::ShouldExclude_Implementation(
    const UCesiumTile* TileObject) {
  if (this->LoadRadius <= 0.0 || !IsValid(TileObject)) {
    return false; // no limit
  }

  FVector ViewLocation;
  if (!GetViewLocation(this->GetWorld(), ViewLocation)) {
    return false; // no camera to measure from -> don't exclude
  }

  // TileObject->Bounds is the tile's bounds in Unreal world space (the adapter
  // refreshes it before each call). Exclude when the nearest point of the tile
  // sphere is beyond the radius (meters -> cm).
  const FBoxSphereBounds& B = TileObject->Bounds;
  const double RadiusCm = this->LoadRadius * 100.0;
  const double NearestDist =
      FVector::Dist(ViewLocation, B.Origin) - B.SphereRadius;
  return NearestDist > RadiusCm;
}
