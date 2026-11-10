# Low frame rate on Linux (2026-11-10)

## Question

On a Fedora 44 laptop (GNOME on Wayland) Rechimped felt slow: low frame rates. Why?

## Machine

- GPUs: Intel UHD Graphics 630 (CoffeeLake-H GT2, i915, Mesa 26.2.6) and an NVIDIA Quadro P2000 Mobile on
  nouveau. Vulkan exposes the Intel GPU and llvmpipe; NVK lists no device for the P2000.
- Display: one internal panel, 1920×1080 at 60.05 Hz, scale 1.
- Power: on battery (`/sys/class/power_supply/AC*/online` = 0), power profile `performance`.
- Intel GPU clocks: 350–1150 MHz.

## Method

- Which GPU: gpui (`gpui-wgpu`) sorts adapters by compositor GPU, then device type, then backend, and
  logs nothing because the app installs no logger. Instead I checked the running process: it uses
  `/dev/dri/renderD128` (Intel, 00:02.0) and touches `renderD129` (NVIDIA) only while enumerating, and
  only Vulkan drivers are loaded (no GL). So it renders on the Intel GPU over Vulkan.
- Frame rate: a temporary probe in `Fx::begin_frame` printed FPS and the CPU time per frame that
  `Fx::end_frame` already measures (render + paint up to the end of paint). This doesn't include gpui's own
  scene submission and present. The test scene was the login screen, which animates continuously. I got
  there without network access by pointing `XDG_CONFIG_HOME` at an empty directory and setting
  `TIMECHIMP_REFRESH_TOKEN=` (empty). Window: 1200×800.
- Effects bisected with a temporary `RECHIMPED_SKIP` switch: aurora orbs, ribbons, sparks, and the login
  card's `glow_frame`.
- Total process CPU from `/proc/<pid>/stat`, GPU clock from `/sys/class/drm/card1/gt_act_freq_mhz`.

The probe and the switch were removed afterwards.

## Results

### Debug build (`cargo build`)

| Effects | FPS | CPU in render/paint | Process CPU |
|---|---|---|---|
| all on | ~30 | ~6 ms | ~30% of a core (~10 ms/frame) |
| all off | ~34 | ~4.4 ms | — |

Idle main screen: about 1% CPU, no frames requested, as intended.

### Release build (`cargo build --release`)

| Effects off | FPS | CPU in render/paint |
|---|---|---|
| none | 30 | ~1 ms |
| orbs | 54 | ~1.1 ms |
| ribbons | 49 | ~0.8 ms |
| sparks | 46 | ~1.0 ms |
| glow | 48 | ~1.2 ms |
| orbs + glow | 60 | ~1.0 ms |
| orbs + sparks | 60 | ~1.1 ms |
| ribbons + sparks + glow | 60 | ~0.9 ms |
| all | 60 | <1 ms |

While the release build animated, the Intel GPU ran at roughly 770–970 MHz, below its 1150 MHz maximum.

## Analysis

- **Frame pacing.** On Wayland gpui presents with `Mailbox` and paces itself with `wl_surface.frame`
  callbacks. A frame that isn't ready in time for a vblank waits for the next one. So a scene that needs a
  bit more than 16.7 ms doesn't run at 55 fps; it drops straight to 30.
- **Release: GPU-bound.** CPU per frame is ~1 ms, but the login effects together need a little more GPU
  time than a 60 Hz frame allows on the UHD 630. No single effect is to blame: removing any one helps
  partly, and removing the orbs (the largest share) plus any one other gets back to 60. Cost grows with the
  window's pixel count, so a larger or maximized window is worse.
- **Debug: CPU-bound as well.** Unoptimized gpui/wgpu take ~10 ms of CPU per frame, enough to miss the
  vblank even with every effect off. Running via `cargo run` therefore stays near 30 fps on any animating
  screen.
- **Battery.** The GPU wasn't at full clock, which pushes a scene that is already close to the limit over
  it. It contributes but isn't the root cause. Not yet tested: the release build on mains power, which
  would show how much of the gap the battery accounts for.

## Options (none applied)

1. Make the aurora cheaper, so it fits in a frame on integrated GPUs with margin: fewer orbs, a smaller
   blur radius (the shadow quad grows with the blur), or fewer sparks (each spark is a separate shadow
   plus a quad).
2. Let it run at 30 fps on purpose: it moves slowly enough that 30 fps would look the same.
3. Make debug builds fast with `[profile.dev.package."*"] opt-level = 2` in the workspace `Cargo.toml`.
   Dependencies get optimized and our own crates stay quick to build and debug. Proposed and declined for
   now.
