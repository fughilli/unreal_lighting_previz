// Napa previz addition: distance-based tile excluder.

#pragma once

#include "CesiumTileExcluder.h"
#include "CoreMinimal.h"
#include "CesiumLoadRadiusExcluder.generated.h"

/**
 * A Cesium tile excluder that only loads tiles within a radius of the camera.
 * Add this component to a Cesium3DTileset actor and set LoadRadius. Tiles whose
 * bounds lie entirely beyond the radius are excluded from selection, so
 * cesium-native never fetches them — useful for previz on a slow connection
 * where the distant landscape isn't needed.
 */
UCLASS(ClassGroup = (Cesium), meta = (BlueprintSpawnableComponent))
class CESIUMRUNTIME_API UCesiumLoadRadiusExcluder : public UCesiumTileExcluder {
  GENERATED_BODY()

public:
  /** Load radius around the camera, in meters. Tiles fully beyond this are not
   * loaded. 0 disables the limit (loads everything). */
  UPROPERTY(
      EditAnywhere,
      BlueprintReadWrite,
      Category = "Cesium",
      meta = (ClampMin = "0.0"))
  double LoadRadius = 200.0;

  virtual bool
  ShouldExclude_Implementation(const UCesiumTile* TileObject) override;
};
