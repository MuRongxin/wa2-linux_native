#!/usr/bin/env python3
"""Inject XTEST key events into the wa2-native window on a given X display.

Used to verify the SDL keyboard path headlessly (Xvfb has no real input):
finds the game window, focuses it, then sends the requested keys.

Usage: xtest_keys.py <display> <Key,Key,...>
  e.g. xtest_keys.py :94 Down,Down,Return
"""
import sys
import time

from Xlib import X, XK, display
from Xlib.ext import xtest


def main() -> int:
    disp_name = sys.argv[1] if len(sys.argv) > 1 else ":94"
    keys = sys.argv[2].split(",") if len(sys.argv) > 2 else ["Down"]

    d = display.Display(disp_name)
    root = d.screen().root

    target = None
    for w in root.query_tree().children:
        try:
            name = w.get_wm_name()
        except Exception:
            name = None
        if name and "wa2" in name.lower():
            target = w
            break
    if target is None:
        print("wa2-native window not found")
        return 1
    print(f"focusing window 0x{target.id:x} ({target.get_wm_name()})")
    d.set_input_focus(target, X.RevertToParent, X.CurrentTime)
    d.sync()
    time.sleep(0.4)

    for k in keys:
        ks = XK.string_to_keysym(k)
        kc = d.keysym_to_keycode(ks)
        if kc == 0:
            print(f"no keycode for {k}")
            continue
        xtest.fake_input(d, X.KeyPress, kc)
        d.sync()
        time.sleep(0.08)
        xtest.fake_input(d, X.KeyRelease, kc)
        d.sync()
        print(f"sent {k} (keycode {kc})")
        time.sleep(0.4)
    print("done")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())