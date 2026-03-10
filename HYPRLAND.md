# Hyprland Window Rules for HyprBoard

Add to `~/.config/hypr/hyprland.conf`:

```conf
# Float and pin HyprBoard (always-on-top overlay)
windowrulev2 = float, class:^(hyprboard)$
windowrulev2 = pin, class:^(hyprboard)$

# Semi-transparent background
windowrulev2 = opacity 0.95 0.85, class:^(hyprboard)$

# No border for cleaner look
windowrulev2 = noborder, class:^(hyprboard)$
```

## Optional rules

```conf
# Start on specific workspace
windowrulev2 = workspace 10 silent, class:^(hyprboard)$

# Custom size
windowrulev2 = size 1200 800, class:^(hyprboard)$

# Center on screen
windowrulev2 = center, class:^(hyprboard)$
```
