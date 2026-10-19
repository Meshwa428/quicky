#!/usr/bin/env python3
"""Virtual mouse for scripted demos. Reads one command per line from FIFO,
answers "ok" on FIFO.ack when it's done:
   glide X Y [MS] | click X Y [MS] | drag X1 Y1 X2 Y2 [MS] | down | up | scroll N | rclick X Y
"""
import os, socket, sys, time
from evdev import UInput, ecodes as e

FIFO = sys.argv[1]
ACK = FIFO + ".ack"
SOCK = os.path.join(os.environ["XDG_RUNTIME_DIR"], "hypr", os.environ["HYPRLAND_INSTANCE_SIGNATURE"], ".socket.sock")

# Keys come from this same device: a separate virtual keyboard (wtype makes
# a new one per press) appearing mid-drag makes Hyprland cancel the drag.
KEYS = {"Alt_L": e.KEY_LEFTALT, "Control_L": e.KEY_LEFTCTRL, "Shift_L": e.KEY_LEFTSHIFT,
        "Escape": e.KEY_ESC, "space": e.KEY_SPACE, "Return": e.KEY_ENTER, "Down": e.KEY_DOWN,
        "l": e.KEY_L, "c": e.KEY_C}
ui = UInput({e.EV_KEY: [e.BTN_LEFT, e.BTN_RIGHT, e.BTN_MIDDLE] + list(KEYS.values()),
             e.EV_REL: [e.REL_X, e.REL_Y, e.REL_WHEEL]}, name="quicky-demo-pointer")

def where():
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(SOCK); s.sendall(b"cursorpos"); out = s.recv(64).decode(); s.close()
    x, y = out.split(",")
    return float(x), float(y)

def nudge(dx, dy):
    ui.write(e.EV_REL, e.REL_X, int(dx)); ui.write(e.EV_REL, e.REL_Y, int(dy)); ui.syn()

import math, random

def path(x0, y0, x1, y1):
    """A hand-like curve: a cubic Bézier whose control points sit off to
    one side of the straight line, by an amount that grows with distance."""
    d = math.hypot(x1 - x0, y1 - y0)
    if d < 1: return lambda k: (x1, y1)
    nx, ny = -(y1 - y0) / d, (x1 - x0) / d          # unit normal
    side = random.choice((-1, 1))
    b1 = side * d * random.uniform(0.06, 0.18)
    b2 = side * d * random.uniform(0.02, 0.12)
    c1 = (x0 + (x1 - x0) * 0.3 + nx * b1, y0 + (y1 - y0) * 0.3 + ny * b1)
    c2 = (x0 + (x1 - x0) * 0.75 + nx * b2, y0 + (y1 - y0) * 0.75 + ny * b2)
    def at(k):
        u = 1 - k
        return (u**3 * x0 + 3 * u * u * k * c1[0] + 3 * u * k * k * c2[0] + k**3 * x1,
                u**3 * y0 + 3 * u * u * k * c1[1] + 3 * u * k * k * c2[1] + k**3 * y1)
    return at

class Runaway(Exception):
    """The cursor reading stopped following the nudges (it can freeze
    during a drag), or the pointer left the area of the move."""

class Watch:
    """Safety for one move: the pointer must stay inside the box around the
    move, and the reported position must keep up with what was sent."""
    def __init__(self, pts, margin=60):
        xs, ys = [p[0] for p in pts], [p[1] for p in pts]
        self.box = (min(xs) - margin, min(ys) - margin, max(xs) + margin, max(ys) + margin)
        self.last = None
        self.unanswered = 0     # px sent since the reading last changed

    def check(self, cx, cy, sent):
        l, t, r, b = self.box
        if not (l <= cx <= r and t <= cy <= b):
            raise Runaway("left the move's area")
        if self.last == (cx, cy):
            self.unanswered += sent
            if self.unanswered > 40:
                raise Runaway("cursor reading stopped changing")
        else:
            self.unanswered = 0
        self.last = (cx, cy)

def follow(at, dur, watch):
    """Walk the curve with a minimum-jerk speed profile (slow start, quick
    middle, gentle landing), correcting against the real cursor."""
    start = time.time()
    sent = 0
    while True:
        t = min(1.0, (time.time() - start) / dur)
        k = 10 * t**3 - 15 * t**4 + 6 * t**5
        tx, ty = at(k)
        cx, cy = where()
        watch.check(cx, cy, sent)
        dx, dy = tx - cx, ty - cy
        # Small events dodge pointer acceleration.
        n = max(1, int(max(abs(dx), abs(dy)) / 6))
        for _ in range(n):
            nudge(round(dx / n), round(dy / n))
        sent = abs(dx) + abs(dy)
        if t >= 1.0: break
        time.sleep(0.008)

def glide(x, y, ms=None):
    """Move to (x, y) the way a hand does. Long moves overshoot a touch and
    come back. `ms` (or a distance-based default) sets the pace."""
    x0, y0 = where()
    d = math.hypot(x - x0, y - y0)
    dur = (ms if ms else 220 + 0.55 * d) / 1000 * random.uniform(0.9, 1.1)
    # The curve bulges sideways by up to ~20% of the distance.
    bulge = d * 0.22
    watch = Watch([(x0, y0), (x, y)], margin=60 + bulge)
    try:
        if d > 220:
            over = random.uniform(0.02, 0.045)
            ox, oy = x + (x - x0) * over + random.uniform(-4, 4), y + (y - y0) * over + random.uniform(-4, 4)
            follow(path(x0, y0, ox, oy), dur * 0.88, watch)
            follow(path(ox, oy, x, y), 0.12 + dur * 0.12, watch)
        else:
            follow(path(x0, y0, x, y), dur, watch)
        # Events land a moment after they're sent: settle only once the
        # cursor reads exactly right twice in a row.
        good, sent = 0, 0
        for _ in range(80):
            time.sleep(0.012)
            cx, cy = where()
            watch.check(cx, cy, sent)
            dx, dy = round(x - cx), round(y - cy)
            if dx == 0 and dy == 0:
                good += 1
                if good == 2: break
                sent = 0
                continue
            good = 0
            sx, sy = max(-3, min(3, dx)), max(-3, min(3, dy))
            nudge(sx, sy)
            sent = abs(sx) + abs(sy)
    except Runaway as why:
        # Stop dead rather than keep pushing the pointer somewhere wrong.
        print(f"glide to {x:.0f},{y:.0f} stopped: {why}", file=sys.stderr, flush=True)

def button(code, v):
    ui.write(e.EV_KEY, code, v); ui.syn()

time.sleep(0.5)
for p in (FIFO, ACK):
    if not os.path.exists(p): os.mkfifo(p)
import select

def quit_now(*_):
    """Let go of everything and remove the virtual pointer."""
    for b in (e.BTN_LEFT, e.BTN_RIGHT, e.BTN_MIDDLE):
        button(b, 0)
    ui.close()
    os._exit(0)

import signal
signal.signal(signal.SIGTERM, quit_now)
signal.signal(signal.SIGINT, quit_now)

IDLE = 15  # seconds without a command before the helper quits by itself

def ack(ident):
    """Tells the script a command is done. If nobody is listening (the
    script died), give up after a moment instead of blocking forever."""
    for _ in range(100):
        try:
            k = os.open(ACK, os.O_WRONLY | os.O_NONBLOCK)
        except OSError:
            time.sleep(0.02)
            continue
        os.write(k, (ident + "\n").encode()); os.close(k)
        return

def lines(fd):
    """Commands from the FIFO; ends (and the helper quits) when idle."""
    buf = b""
    while True:
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            yield line.decode()
        if not select.select([fd], [], [], IDLE)[0]:
            quit_now()
        buf += os.read(fd, 4096)

# Opened read-write, the FIFO never reaches EOF, so no command can slip in
# while it is being reopened.
fd = os.open(FIFO, os.O_RDWR)
if True:
    while True:
        for line in lines(fd):
            a = line.split()
            if not a: continue
            # Each command starts with an id, echoed back on the ack.
            ident, a = a[0], a[1:]
            if not a: continue
            c = a[0]
            n = [] if c == "key" else [float(v) for v in a[1:]]
            # A command missing its coordinates is skipped, not fatal.
            if c in ("glide", "click", "rclick") and len(n) < 2 or c == "drag" and len(n) < 4:
                ack(ident)
                continue
            # A duration of 0 (or none) means "at a natural pace".
            ms = lambda i: (n[i] or None) if len(n) > i else None
            if c == "glide": glide(n[0], n[1], ms(2))
            elif c == "down": button(e.BTN_LEFT, 1)
            elif c == "up": button(e.BTN_LEFT, 0)
            elif c == "click":
                glide(n[0], n[1], ms(2)); time.sleep(random.uniform(0.06, 0.14))
                button(e.BTN_LEFT, 1); time.sleep(random.uniform(0.06, 0.1)); button(e.BTN_LEFT, 0)
            elif c == "rclick":
                glide(n[0], n[1]); button(e.BTN_RIGHT, 1); time.sleep(0.07); button(e.BTN_RIGHT, 0)
            elif c == "drag":
                glide(n[0], n[1]); time.sleep(random.uniform(0.08, 0.15)); button(e.BTN_LEFT, 1); time.sleep(0.12)
                glide(n[2], n[3], ms(4)); time.sleep(0.12); button(e.BTN_LEFT, 0)
            elif c == "key":
                code = KEYS.get(a[1])
                if code:
                    button(code, 1); time.sleep(0.06); button(code, 0)
            elif c == "scroll":
                ui.write(e.EV_REL, e.REL_WHEEL, int(n[0])); ui.syn()
            elif c == "quit":
                ui.close(); sys.exit(0)
            ack(ident)
