-- Example OBSBOT Tiny 3 keybindings for Omarchy (Hyprland-in-Lua).
--
-- Copy the o.bind lines you want into ~/.config/hypr/bindings.lua. Omarchy
-- auto-reloads on save; validate with `hyprctl reload && hyprctl configerrors`.
--
-- These use SUPER+ALT+<key>, which are unbound in a stock Omarchy install.
-- If you have bound any of them, unbind first: hl.unbind("SUPER + ALT + C").

-- Camera sleep toggle — the headline feature. Notifies the resulting state.
o.bind("SUPER + ALT + C", "Camera sleep toggle",
  [[sh -c 'notify-send -a OBSBOT "OBSBOT Tiny 3" "Camera now $(t3ctl toggle)"']])

-- Recenter the gimbal (park).
o.bind("SUPER + ALT + R", "Camera recenter", "t3ctl recenter")

-- Toggle AI subject tracking. Notifies the resulting state.
o.bind("SUPER + ALT + T", "Camera tracking toggle",
  [[sh -c 'notify-send -a OBSBOT "OBSBOT Tiny 3" "Tracking $(t3ctl track toggle)"']])
