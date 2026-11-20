# YARG.Core

Common backend code used throughout various YARG projects/services.

# ⚠ THIS IS NOT THE GAME! ⚠

If you are looking for YARG itself, [you may find it here](https://github.com/YARC-Official/YARG).

## ✍️ Contributing

If you want to contribute, please feel free! Please join [our Discord](https://discord.gg/sqpu4R552r) for easier communication and discussion.

## 🛡️ License

YARG.Core is licensed under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html) (or later) - see the [`LICENSE`](LICENSE) file for details.

## 🧰 External Licenses

Some libraries/assets are packaged **within** the source code and have licenses that must be included.

| Library | License |
| --- | --- |
| [Moonscraper](https://github.com/FireFox2000000/Moonscraper-Chart-Editor) | [BSD 3-Clause License](https://github.com/FireFox2000000/Moonscraper-Chart-Editor/blob/master/LICENSE)

Please note that other libraries are **not** packaged within the source code, and are to be install by NuGet.

## 📦 External Assets and Libraries

| Link | Type | Use |
| --- | --- | --- |
| [PlasticBand](https://github.com/TheNathannator/PlasticBand) | Reference | Controller Support Info
| [GuitarGame_ChartFormats](https://github.com/TheNathannator/GuitarGame_ChartFormats) | Reference | File Format Documentation
| [DryWetMidi](https://www.nuget.org/packages/Melanchall.DryWetMidi) | Library | Parsing `.mid` Files
| [Moonscraper](https://github.com/FireFox2000000/Moonscraper-Chart-Editor) | Library | Parsing `.chart` Files
| [NUnit](https://docs.nunit.org/articles/nunit/intro.html) | Library | Unit tests

## Dance and 4-key fork

`main` in this fork starts from YARG's pinned core revision
`3beb94e526558134145bcd3e409428f759001a40`; it retains that API for the matching
Unity game. Dance/4-key instrument and game-mode IDs are appended without changing
existing serialized values. `DanceLibrary.Refresh` attaches standalone SM/SSC
entries and YACE-pad/YACE-keyboard sidecars to the normal song cache. Loaders retain
the band's sync track and convert independent dance clocks for song.ini delay.

Parsing, StepMania timing, rendering positions and note judgments live in the MIT
Rust crate under `Native/dance`, which vendors YACE's portable simfile library.
`YARG.Core/Dance` contains the .NET data/ABI and engine adapters. Attribution is in
`Native/dance/NOTICE.md` and `Native/dance/licenses`.

Before running the .NET tests, build and test the host's native library:

```sh
cargo test --locked --manifest-path Native/dance/Cargo.toml
cargo build --release --locked --manifest-path Native/dance/Cargo.toml
dotnet test YARG.Core.UnitTests/YARG.Core.UnitTests.csproj
```

Native build output is copied into the test assembly's directory by its project
file. Integration tests call the Rust fixture generator and verify real simfile
loading, five concurrent engines, offsets, cache invalidation and replay round trips.
Unity installation, controls and the exported song-directory layout are documented
in the parent game's `docs/DANCE.md`.
