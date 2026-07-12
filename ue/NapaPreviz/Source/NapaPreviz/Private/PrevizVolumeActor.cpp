// Copyright Napa Lighted Art Festival previz.
#include "PrevizVolumeActor.h"

#include "PrevizVolumeComponent.h"
#include "Components/SceneComponent.h"

APrevizVolumeActor::APrevizVolumeActor()
{
    // A scene root the component's InstancedStaticMesh can attach under.
    USceneComponent* Root = CreateDefaultSubobject<USceneComponent>(TEXT("Root"));
    SetRootComponent(Root);

    Volume = CreateDefaultSubobject<UPrevizVolumeComponent>(TEXT("Volume"));
}
