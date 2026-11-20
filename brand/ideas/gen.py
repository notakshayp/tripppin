"""Linux-flavoured logo ideas for tripppin: 200x200, one colour, cutouts transparent."""
import json, math, sys

OUT = sys.argv[1]
TAU = math.pi * 2
I = []


def add(name, family, idea, body):
    I.append({"id": f"x{len(I) + 1}", "name": name, "family": family, "idea": idea, "body": body})


def circ(cx, cy, r, ccw=False):
    s = 0 if ccw else 1
    return f"M{cx - r:.1f} {cy:.1f}a{r} {r} 0 1 {s} {2 * r:.1f} 0a{r} {r} 0 1 {s} {-2 * r:.1f} 0Z"


def rr(x, y, w, h, r, ccw=False):
    if ccw:
        return (f"M{x + r} {y}A{r} {r} 0 0 0 {x} {y + r}V{y + h - r}A{r} {r} 0 0 0 {x + r} {y + h}"
                f"H{x + w - r}A{r} {r} 0 0 0 {x + w} {y + h - r}V{y + r}A{r} {r} 0 0 0 {x + w - r} {y}Z")
    return (f"M{x + r} {y}H{x + w - r}A{r} {r} 0 0 1 {x + w} {y + r}V{y + h - r}A{r} {r} 0 0 1 {x + w - r} {y + h}"
            f"H{x + r}A{r} {r} 0 0 1 {x} {y + h - r}V{y + r}A{r} {r} 0 0 1 {x + r} {y}Z")


def path(d, evenodd=True):
    rule = ' fill-rule="evenodd"' if evenodd else ""
    return f'<path{rule} d="{d}"/>'


def line(d, w, cap="round", color="currentColor"):
    return f'<path d="{d}" fill="none" stroke="{color}" stroke-width="{w}" stroke-linecap="{cap}" stroke-linejoin="round"/>'


def mask(mid, keep, cut):
    return (f'<mask id="{mid}" maskUnits="userSpaceOnUse" x="0" y="0" width="200" height="200">'
            f'<rect width="200" height="200" fill="#fff"/>{cut}</mask><g mask="url(#{mid})">{keep}</g>')


def star4(cx, cy, r, k=0.28):
    pts = []
    for i in range(8):
        rad = r if i % 2 == 0 else r * k
        a = -math.pi / 2 + TAU * i / 8
        pts.append((cx + rad * math.cos(a), cy + rad * math.sin(a)))
    return "M" + " ".join(f"{x:.1f} {y:.1f}" for x, y in pts) + "Z"


W = 22  # house stroke weight

# ---------------- prompt & cursor ----------------
add("prompt", "prompt", "The shell prompt, heavy and rounded: a chevron and a block cursor.",
    line("M38 58 L86 100 L38 142", W) + path(rr(104, 128, 64, 22, 6)))
add("prompt bubble", "prompt", "A speech bubble whose voice is a prompt: talk to the shell.",
    mask("m-pb", path(rr(16, 22, 168, 130, 36) + "M48 150 L40 186 L84 150 Z"),
         line("M58 64 L92 88 L58 112", 18, color="#000") + f'<path d="{rr(104, 102, 44, 16, 5)}" fill="#000"/>'))
add("spark cursor", "prompt", "A block cursor with an AI spark beside it.",
    path(rr(30, 84, 74, 96, 10)) + path(star4(144, 56, 40)))
add("typing", "prompt", "A prompt followed by three dots: it's thinking.",
    line("M26 64 L66 100 L26 136", W) + path(circ(102, 100, 14) + circ(140, 100, 14) + circ(178, 100, 14)))
add("brackets", "prompt", "Two brackets holding a single point: a window waiting for you.",
    line("M64 30 H36 V170 H64 M136 30 H164 V170 H136", W, "butt") + path(circ(100, 100, 22)))
add("caret t", "prompt", "A lowercase t whose crossbar is a blinking cursor.",
    line("M84 26 V146 Q84 176 116 176 H130", W) + path(rr(50, 62, 100, 22, 6)))
add("keycap", "prompt", "A keycap with a prompt on it: the one key you need.",
    path(rr(20, 20, 160, 160, 34) + rr(38, 32, 124, 120, 22, True)) + line("M70 72 L100 94 L70 116", 16) + path(rr(108, 108, 30, 12, 4)))

# ---------------- the name ----------------
p = lambda x: f"M{x} 176 V60 " + f"M{x} 98 A38 38 0 1 1 {x} 98.1"
add("ppp", "name", "The triple p of tripppin, three letters linked into one chain.",
    "".join(line(f"M{x} 70 V182", 17) + line(f"M{x} 96 a28 28 0 1 1 0 0.01", 17) for x in (32, 78, 124)))
add("tri dots", "name", "∴: three points in a triangle, for tri.", path(circ(100, 50, 30) + circ(50, 140, 30) + circ(150, 140, 30)))
add("three chevrons", "name", "››› : three steps forward, three ps.", line("M30 56 L70 100 L30 144 M78 56 L118 100 L78 144 M126 56 L166 100 L126 144", 18))
add("t seal", "name", "A lowercase t cut out of a rounded square: the app icon.",
    mask("m-ts", path(rr(18, 18, 164, 164, 42)), line("M92 44 V136 Q92 158 116 158 H126", 24, color="#000") + line("M62 82 H138", 24, color="#000")))
add("orb t", "name", "A t whose top is a round orb: the assistant sitting on the letter.",
    path(circ(92, 46, 24)) + line("M92 82 V146 Q92 174 120 174 H134 M60 104 H132", W))
trefoil = []
for i in range(361):
    t = math.radians(i)
    r = 46 + 32 * math.cos(3 * t)
    trefoil.append((100 + r * math.cos(t - math.pi / 2), 104 + r * math.sin(t - math.pi / 2)))
add("tri loop", "name", "A three-lobed loop drawn in one line.", line("M" + " ".join(f"{x:.1f} {y:.1f}" for x, y in trefoil) + "Z", 18))

# ---------------- eye & orb ----------------
add("glance", "orb", "The current mark, for comparison.",
    '<g transform="rotate(28 100 100)">' + path("M100 22 L140.4 96 A46 46 0 1 1 59.6 96 Z" + circ(100, 130, 30, True)) + path(circ(100, 138, 13)) + "</g>")
add("look", "orb", "A ring with its pupil off to one side: the system is paying attention.", path(circ(100, 100, 84) + circ(100, 100, 58, True)) + path(circ(124, 88, 24)))
add("status light", "orb", "A capsule with one lit point, like a power LED that listens.", path(rr(30, 66, 140, 68, 34) + rr(48, 82, 104, 36, 18, True)) + path(circ(128, 100, 12)))
add("halo", "orb", "A dot under a floating arc: a mind above the machine.", path(circ(100, 124, 40)) + line("M40 66 Q100 20 160 66", 18))
add("orbit", "orb", "A tilted ring with one body travelling on it: a running process.",
    '<g transform="rotate(-24 100 100)">' + path("M14 100 A86 40 0 1 0 186 100 A86 40 0 1 0 14 100 Z M34 100 A66 26 0 1 1 166 100 A66 26 0 1 1 34 100 Z") + "</g>" + path(circ(100, 100, 30)) + path(circ(160, 66, 13)))

# ---------------- unix ----------------
add("tilde", "unix", "~ : home. One bold wave.", line("M28 116 C 52 64 82 64 100 100 C 118 136 148 136 172 84", 26))
add("root", "unix", "/ : the root of everything, with a dot at its base.", line("M140 26 L74 154", 26) + path(circ(58, 176, 14)))
sp = []
for i in range(240):
    t = i / 239 * 2.6 * math.pi
    r = 5 * math.exp(0.33 * t)
    sp.append((96 + r * math.cos(t + math.pi), 104 + r * math.sin(t + math.pi)))
add("shell", "unix", "A shell, literally: one line spiralling out from the centre.",
    line("M" + " ".join(f"{x:.1f} {y:.1f}" for x, y in sp), 17))
add("daemon", "unix", "∞ : a service that's always running.", line("M100 100 C 76 64 30 64 30 100 C 30 136 76 136 100 100 C 124 64 170 64 170 100 C 170 136 124 136 100 100 Z", 22))
add("process tree", "unix", "One parent, three children: a process tree.",
    line("M100 46 V96 M100 96 H44 V140 M100 96 V140 M100 96 H156 V140", 14) + path(circ(100, 40, 22) + circ(44, 156, 18) + circ(100, 156, 18) + circ(156, 156, 18)))
add("hash", "unix", "# : the root prompt, softened into a grid with one open cell.",
    mask("m-hash", line("M70 30 L58 170 M130 30 L118 170 M30 72 H172 M26 128 H168", 24), path(rr(78, 78, 44, 44, 6)).replace("<path", '<path fill="#000"')))
add("bang", "unix", "! : run it now. A bold mark with a round point.", path(rr(82, 20, 36, 110, 18)) + path(circ(100, 166, 20)))

# ---------------- system ----------------
add("layers", "system", "Three stacked layers, the top one lifted: kernel, system, you.",
    path("M100 34 L176 72 L100 110 L24 72 Z") + path("M24 100 L44 90 L100 118 L156 90 L176 100 L100 138 Z") + path("M24 128 L44 118 L100 146 L156 118 L176 128 L100 166 Z"))
add("kernel", "system", "A hexagonal nut with a point at its core: the kernel.",
    path("M100 16 L172 58 L172 142 L100 184 L28 142 L28 58 Z" + circ(100, 100, 40, True)) + path(circ(100, 100, 16)))
add("cube", "system", "An isometric cube with one face open.",
    path("M100 18 L172 60 L100 102 L28 60 Z") + path("M28 74 L92 112 L92 186 L28 148 Z") + line("M108 112 L172 74 L172 148 L108 186 Z", 10))
add("power spark", "system", "The power symbol, with a spark where the line would be.",
    line("M60 66 A60 60 0 1 0 140 66", 22) + path(star4(100, 50, 40)))
add("window", "system", "A window with the prompt inside: the app icon.",
    path(rr(18, 26, 164, 148, 26) + rr(34, 60, 132, 98, 12, True)) + path(circ(42, 44, 6) + circ(60, 44, 6) + circ(78, 44, 6), False) + line("M60 88 L84 108 L60 128", 14) + path(rr(94, 120, 40, 12, 4)))
add("spark", "system", "A four-point spark: the AI, alone.", path(star4(100, 100, 86, 0.22)))
add("spark ring", "system", "A spark inside a ring: intelligence, contained.", path(circ(100, 100, 88) + circ(100, 100, 70, True)) + path(star4(100, 100, 52, 0.24)))
add("bubbles", "system", "Two overlapping speech bubbles: you and your computer.",
    path(rr(16, 30, 112, 86, 28) + "M40 114 L36 142 L66 114 Z") +
    mask("m-bub", path(rr(76, 78, 108, 84, 28) + "M160 160 L166 186 L138 160 Z"), f'<path d="{rr(10, 24, 124, 98, 34)}" fill="#000"/>'))
add("voice", "system", "Five voice bars, the middle one tallest: speak to it.",
    "".join(path(rr(x - 10, 100 - h / 2, 20, h, 10)) for x, h in ((40, 50), (70, 96), (100, 150), (130, 96), (160, 50))))

json.dump(I, open(OUT, "w"))
print(len(I), "ideas")
