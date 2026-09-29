# Perfect Dark: Combat Simulator, in Rust

A faithful recreation of the **Combat Simulator** from Perfect Dark (N64, 2000), written in Rust. It is not an emulator and not a port of the PC port. It ports the game's own functions from the [Perfect Dark decompilation](https://github.com/n64decomp/perfect_dark), one at a time, under their original names and units, and aims to behave exactly as the NTSC-final (US v1.1) cartridge does, including its bugs.

What's in:

- **The Perfect Menu**: the agent select, the file manager and every Combat Simulator setup dialog, rendered pixel-for-pixel on a software N64 RDP
- **All 16 multiplayer arenas**, with doors, lifts, glass, skies and PD's own simulant waypoint graphs
- **All six scenarios**: Combat, Hold the Briefcase, Hacker Central, Pop a Cap, King of the Hill and Capture the Case
- **Simulants**: every type and difficulty, plus team commands
- **The full MP arsenal**: hitscan and projectile weapons, explosives, secondary functions, the rocket launcher's lock-on and the N-Bomb
- **Match flow**: limits, teams, the pause menu, the end screens, and **the 30 challenges**
- **Presentation**: up to 4-player split screen, radar, shields, the death camera, rumble, music (played by an N64 audio-library port), and an optional N64 video and CRT TV filter
- **Saves**: kept in PD's own file system on a 2 KB Game Pak EEPROM image, so a 16 Kbit `.eep` from an emulator loads

What's not in: the solo campaign, co-op, counter-op and the Carrington Institute.

The code lives in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Progress and plans live in [docs/MILESTONES.md](docs/MILESTONES.md).

## You need your own ROM

This repository contains no ROM. To build the game data (models, textures, animations, sounds, music, stages, text), you need a dump of **your own cartridge**. It must be this exact version:

| | |
|---|---|
| Game | Perfect Dark, **USA, revision 1** (v1.1, `NUS-NPDE-USA`), called `ntsc-final` in the decomp |
| Format | `.z64`, big-endian (the native byte order) |
| MD5 | `e03b088b6ac9e0080440efed07c1e40f` |

The US v1.0 release, the PAL and Japanese releases and the betas will **not** work. Their data sits at different offsets. Check your dump before you start:

```powershell
Get-FileHash -Algorithm MD5 -LiteralPath "path\to\your\rom.z64"   # PowerShell
certutil -hashfile "path\to\your\rom.z64" MD5                      # cmd
md5sum path/to/your/rom.z64                                        # Linux / macOS / Git Bash
```

Use `-LiteralPath` in PowerShell because ROM file names often contain `[!]`, and PowerShell treats square brackets as wildcards.

**If the hash doesn't match**, your dump may be in another byte order. `.v64` dumps have each pair of bytes swapped. `.n64` dumps have each 4-byte word reversed. This converts either one to `.z64`:

```python
# to_z64.py: python to_z64.py <in> <out.z64>
import sys
data = bytearray(open(sys.argv[1], "rb").read())
magic = bytes(data[:4])
if magic == b"\x37\x80\x40\x12":            # .v64: swap each 16-bit pair
    data[0::2], data[1::2] = data[1::2], data[0::2]
elif magic == b"\x40\x12\x37\x80":          # .n64: reverse each 32-bit word
    for i in range(0, len(data), 4):
        data[i:i+4] = data[i:i+4][::-1]
elif magic != b"\x80\x37\x12\x40":
    sys.exit("not an N64 ROM")
open(sys.argv[2], "wb").write(data)
```

Then check the MD5 again. If it still doesn't match, you have a different release.

## Setup

### 1. Install the tools

- **Rust**, through [rustup](https://rustup.rs). The repo pins Rust 1.92 in `rust-toolchain.toml`, and rustup installs it on the first build. On Windows, also install the Visual Studio C++ Build Tools when rustup offers them.
- **Python 3** (developed on 3.12). The asset exporters use only the standard library.
- **Git**.
- A GPU with Vulkan, DirectX 12 or Metal.

The game is developed and tested on Windows 10. Nothing in it is Windows-only, but Linux and macOS haven't been tried.

### 2. Get the decomp and extract your ROM into it

The exporters read the Perfect Dark decomp, and the decomp's own extractor unpacks the ROM into it. Clone it into `reference/pd-decomp`, then check out the commit this repo's assets were built from:

```
git clone https://github.com/n64decomp/perfect_dark reference/pd-decomp
git -C reference/pd-decomp checkout 169ed48bdcbfb3b568b028bd5bebb27680073514
```

Copy your ROM into the decomp's root folder and name it **`pd.ntsc-final.z64`**:

```
reference/pd-decomp/pd.ntsc-final.z64
```

Run the extractor from inside that folder. It adds about 230 MB:

```powershell
# PowerShell
cd reference/pd-decomp
$env:ROMID = "ntsc-final"; python tools/extract
cd ../..
```

```sh
# bash / Git Bash
cd reference/pd-decomp
ROMID=ntsc-final python tools/extract
cd ../..
```

`reference/` is gitignored, so none of this gets committed. You can keep the decomp somewhere else and point `PD_DECOMP_DIR` at it.

### 3. Build the game data

```
python tools/pd-assets/build_assets.py
```

This writes `assets/` (about 40 MB) and regenerates the Rust tables that come from the decomp (`crates/pd_core/src/ids.rs` and friends). It takes about 25 seconds and gives the same bytes every time. Its `MANIFEST.json` records the decomp commit and the ROM's MD5.

If it stops with "is not extracted", step 2 didn't finish. If it says "no decomp", the decomp isn't at `reference/pd-decomp` and `PD_DECOMP_DIR` isn't set.

### 4. Build and run

```
cargo build --release
target/release/perfect_dark
```

The first build takes a few minutes. Later ones take seconds. The game finds `assets/` by walking up from the executable, or you can set `PD_ASSETS` to its path.

Optional: `cargo test --workspace --release` runs the test suite against your freshly built assets. The menu tests compare against golden images, so they also check that your data matches.

## Playing

Power on opens the agent select, as the cartridge does. Create a file, then pick **Combat Simulator**.

| Flag | Does |
|---|---|
| `--combat` | skip straight to the Combat Simulator |
| `--unlock-all` | treat every challenge as completed, which unlocks the whole roster, arenas and weapons |
| `--fresh` | start on a blank Game Pak and don't save |

Saves go to `save/perfect_dark.eep`. Set `PD_SAVE` to use another file.

### Controls

**In a match**, the keyboard and mouse work like the PC port:

| | |
|---|---|
| WASD | move |
| Mouse | look |
| Left mouse button | fire |
| Right mouse button (hold) | aim |
| E or middle mouse button | use |
| R | reload |
| Q | next gun (hold it for the active menu, Q + left mouse button for the previous gun) |
| 1 to 0 | pick a gun by inventory slot |
| Ctrl or C / Space | crouch down / stand up |
| Up / Down arrows | zoom |
| Enter | START (pause) |
| Esc | release the mouse (click to capture it again) |

**In the menus**, the keyboard stands in for an N64 controller:

| | |
|---|---|
| Arrow keys or WASD | D-pad |
| Enter | A |
| Esc | B |
| Space | START |
| Z | Z |
| Q / E | L / R |
| F2, F3, F4 | press START on controllers 2 to 4 (lets more players join without more pads) |

**Gamepads** drive players 1 to 4 with PD's control style 1.1. Buttons are read as a USB N64 controller adapter reports them, so other pads may map oddly.

## For developers

[CLAUDE.md](CLAUDE.md) has the full command list: the headless snapshot tool (`pd_snapshot`, which renders menus, stages, matches and flows to PNG), long probes, offline music renders and the simulant route viewer. The rules the code follows are:

- **The decomp is the spec.** Code ports PD's functions under PD's names and cites `file.c:line` from the decomp.
- **Differences from PD are marked** with `// SUBST:` comments. `rg "SUBST:"` lists every one.
- **The simulation is headless and deterministic.** `pd_core`, `pd_sim` and `pd_menu` never touch the GPU, windowing or audio. `python tools/check_boundaries.py` checks this.
- **Assets are generated.** Never edit `assets/` by hand. Change the exporter in `tools/pd-assets/` and rerun it.

## Credits

This is built on the [n64decomp Perfect Dark decompilation](https://github.com/n64decomp/perfect_dark), with the [PC port](https://github.com/fgsfdsfgs/perfect_dark) as a reference for mouse aim and the audio microcode. Perfect Dark is © Rare Ltd. / Microsoft. This is an unofficial fan project, not affiliated with or endorsed by them.
