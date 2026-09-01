-- OBSBOT Tiny 3 live-preview window rule for Omarchy (Hyprland-in-Lua).
--
-- The `t3-preview` script launches mpv with --wayland-app-id=obsbot-tiny3-preview.
-- This rule floats it as a small, pinned, corner self-view (bottom-right, 40px
-- inset), scaled from monitor height so it takes the same share of any display.
--
-- Install: copy this into ~/.config/hypr/windows.lua and add
--   require("hypr.windows")
-- to ~/.config/hypr/hyprland.lua (after the other require lines). Omarchy
-- auto-reloads on save; validate with `hyprctl reload && hyprctl configerrors`.

o.window({ class = "^obsbot-tiny3-preview$" }, {
  float = true,
  pin = true,
  no_initial_focus = true,
  no_dim = true,
  tag = "-default-opacity",
  opacity = "1 1",
  size = { "(monitor_h*3/10)", "(monitor_h*27/160)" }, -- ~16:9
  move = { "(monitor_w-monitor_h*3/10-40)", "(monitor_h-monitor_h*27/160-40)" },
})
