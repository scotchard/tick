-- Tick: keybinds and window rules for Omarchy's Lua Hyprland config.
-- Add these to ~/.config/hypr/bindings.lua (keybinds) and
-- ~/.config/hypr/hyprland.lua (window rules), then save; Hyprland reloads.

-- Keybinds ------------------------------------------------------------------

-- The list, in a floating terminal. Pressing again focuses the open one.
o.bind("SUPER + SHIFT + T", "Tick", "omarchy-launch-or-focus-tui tick")

-- Quick capture: type a to-do, Enter, gone. Lands in Inbox (Tab picks a list).
o.bind("SUPER + ALT + T", "Tick quick add", "omarchy-launch-or-focus-tui --app-id=tick-capture tick capture")

-- The window version (sheet + sidebar).
o.bind("SUPER + SHIFT + CTRL + T", "Tick window", "omarchy-launch-or-focus '^tick$' tick-gui")

-- Window rules ----------------------------------------------------------------

-- Terminal list: Omarchy's standard floating size (875x600, centred).
o.window("^(org.omarchy.tick)$", { tag = "+floating-window" })

-- Quick capture: a short strip in the middle of the screen.
o.window("^(tick-capture)$", { float = true, center = true, size = { 640, 140 } })

-- Window version: floating, centred, and opaque (it's a reading surface).
-- Delete the opacity lines to keep Omarchy's default see-through look.
o.window("^(tick)$", { float = true, center = true, size = { 720, 680 } })
o.window("^(tick)$", { tag = "-default-opacity" })
o.window("^(tick)$", { opacity = "1 1" })
