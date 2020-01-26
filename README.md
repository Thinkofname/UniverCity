# UniverCity

This is the open source release of the game Univercity
https://store.steampowered.com/app/808160/UniverCity/

This repo does not contain the assets required to run the game,
only the code for the server/client is provided. To run copy
the assets folder from the release of the game into this folder
then:

```bash
cargo run --release
```

Steam integration is disabled in this release behind the feature
flag `steam` due to the GPL licensing but the code is left in
to match the release state of the game.

The credits screen requires the `licenses.json` to be generated.
You can either copy the licenses folder from the release version
of the game or generate it locally with:

```bash
cargo install --git https://github.com/Thinkofname/cargo-license
mkdir licenses
cargo unilicense --packages univercity,univercity_server_dedicated > ./licenses/licenses.json
```