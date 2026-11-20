"""Generates ~48 Indic-themed logo ideas as SVG symbols (200x200, one colour, cutouts transparent)."""
import json, math, sys

OUT = sys.argv[1]
TAU = math.pi * 2


def P(pts, close=True):
    return "M" + " ".join(f"{x:.1f} {y:.1f}" for x, y in pts) + ("Z" if close else "")


def circ(cx, cy, r, ccw=False):
    s = 0 if ccw else 1
    return f"M{cx - r:.1f} {cy:.1f}a{r} {r} 0 1 {s} {2 * r:.1f} 0a{r} {r} 0 1 {s} {-2 * r:.1f} 0Z"


def rot(x, y, deg, ox=100, oy=100):
    a = math.radians(deg)
    x -= ox
    y -= oy
    return ox + x * math.cos(a) - y * math.sin(a), oy + x * math.sin(a) + y * math.cos(a)


def rotpts(pts, deg, ox=100, oy=100):
    return [rot(x, y, deg, ox, oy) for x, y in pts]


def drop(cx, cy, r, d, deg=0, n=48):
    """Teardrop: circle (cx,cy,r) with a point d above the centre, rotated by deg about the centre."""
    th = math.asin(r / d)
    start = -math.pi / 2 + (math.pi / 2 - th)
    pts = [(cx, cy - d)]
    for i in range(n + 1):
        a = start + (TAU - 2 * (math.pi / 2 - th)) * i / n
        pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return rotpts(pts, deg, cx, cy)


def petal(cx, cy, length, width, deg, base=0, n=30):
    """Pointed almond petal from (cx,cy) outward, rotated deg (0 = up)."""
    left, right = [], []
    for i in range(n + 1):
        t = i / n
        w = width * math.sin(math.pi * t) ** 0.9
        y = cy - base - length * t
        left.append((cx - w, y))
        right.append((cx + w, y))
    return rotpts(left + right[::-1], deg, cx, cy)


def bez(p0, p1, p2, p3, t):
    u = 1 - t
    return (u ** 3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t ** 3 * p3[0],
            u ** 3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t ** 3 * p3[1])


def taper(curves, w0, w1, n=50):
    pts = []
    for c in curves:
        pts += [bez(*c, i / n) for i in range(n)]
    pts.append(curves[-1][3])
    L, R = [], []
    for i, p in enumerate(pts):
        q, r = pts[min(i + 1, len(pts) - 1)], pts[max(i - 1, 0)]
        dx, dy = q[0] - r[0], q[1] - r[1]
        d = math.hypot(dx, dy) or 1
        nx, ny = -dy / d, dx / d
        w = (w0 + (w1 - w0) * (i / (len(pts) - 1))) / 2
        L.append((p[0] + nx * w, p[1] + ny * w))
        R.append((p[0] - nx * w, p[1] - ny * w))
    return P(L + R[::-1])


def ring(cx, cy, r1, r0):
    return circ(cx, cy, r1) + circ(cx, cy, r0, True)


def path(d, evenodd=True):
    rule = ' fill-rule="evenodd"' if evenodd else ""
    return f'<path{rule} d="{d}"/>'


def stroke(d, w, cap="round"):
    return f'<path d="{d}" fill="none" stroke="currentColor" stroke-width="{w}" stroke-linecap="{cap}" stroke-linejoin="round"/>'


def mask(mid, keep, cut):
    return (f'<mask id="{mid}" maskUnits="userSpaceOnUse" x="0" y="0" width="200" height="200">'
            f'<rect width="200" height="200" fill="#fff"/>{cut}</mask><g mask="url(#{mid})">{keep}</g>')


def text(t, size, y, family, weight=400, x=100):
    return (f'<text x="{x}" y="{y}" text-anchor="middle" font-family="{family}" font-weight="{weight}" '
            f'font-size="{size}" fill="currentColor">{t}</text>')


I = []  # (id, name, family, idea, body)


def add(name, family, idea, body):
    I.append((f"i{len(I) + 1}", name, family, idea, body))


# ---------------- peacock & feather ----------------
add("mor", "peacock", "A feather eye, upright: teardrop, window, pupil.",
    path(P(drop(100, 120, 56, 108)) + circ(100, 132, 32, True)) + path(circ(100, 140, 14)))
add("glance", "peacock", "The current mark: the feather eye tipped 28°.",
    f'<g transform="rotate(28 100 100)">' + path(P(drop(100, 118, 46, 96)) + circ(100, 130, 30, True)) + path(circ(100, 138, 13)) + '</g>')
f3 = "".join(path(P(drop(*rot(100, 92, a, 100, 176), 28, 70, a + 0)) + circ(*rot(100, 86, a, 100, 176), 11, True)) for a in (-44, 0, 44))
add("tail", "peacock", "Three feather eyes fanning from one point.", f'<g transform="translate(0 6)">{f3}</g>')
quill = taper([((86, 196), (92, 160), (100, 140), (104, 110))], 9, 4) + P(drop(108, 74, 30, 64, 10)) + circ(*rot(108, 80, 10, 108, 74), 12, True)
add("quill", "peacock", "One feather on a curved shaft.", path(quill))
barbs = "".join(f'<path d="M100 {y} L170 {y - 40}" stroke="#000" stroke-width="8"/>' for y in (58, 82, 106, 130))
add("barbs", "peacock", "A feather eye with the barbs cut along one side.",
    mask("m-barbs", path(P(drop(100, 120, 56, 108)) + circ(100, 138, 22, True)), barbs) + path(circ(100, 142, 9)))
neck = taper([((70, 178), (66, 140), (124, 136), (118, 98)), ((118, 98), (115, 76), (96, 72), (100, 58))], 26, 12)
head = circ(108, 52, 13) + "M119 46 L140 54 L119 59 Z"
add("neck", "peacock", "A peacock's neck rising inside a circle.",
    mask("m-neck", path(circ(100, 100, 86)), f'<path d="{neck}{head}" fill="#000"/>') + path(circ(110, 50, 3)))
crest = "".join(path(P(drop(*rot(100, 46, a, 100, 112), 14, 30, a))) + stroke(f"M100 112 L{rot(100, 66, a, 100, 112)[0]:.1f} {rot(100, 66, a, 100, 112)[1]:.1f}", 6) for a in (-34, 0, 34))
add("crest", "peacock", "Only the crest: three feather tips on fine stems, over a dot.", crest + path(circ(100, 150, 24)))

# ---------------- chakra & sun ----------------
spokes = "".join(stroke(f"M{rot(100, 34, a)[0]:.1f} {rot(100, 34, a)[1]:.1f}L{rot(100, 84, a)[0]:.1f} {rot(100, 84, a)[1]:.1f}", 3.5) for a in range(0, 360, 15))
add("ashoka", "chakra", "A 24-spoke wheel: ring, spokes, hub.", path(ring(100, 100, 92, 80)) + spokes + path(circ(100, 100, 16)))
eight = "".join(path(P(petal(100, 100, 68, 8, a, 14))) for a in range(0, 360, 45))
add("dharma", "chakra", "Eight broad spokes in a heavy ring.", path(ring(100, 100, 90, 72)) + eight + path(ring(100, 100, 22, 12)))
saw = []
for i in range(24):
    a0 = TAU * i / 24
    saw += [(100 + 74 * math.cos(a0), 100 + 74 * math.sin(a0)), (100 + 92 * math.cos(a0 + TAU / 24 * .75), 100 + 92 * math.sin(a0 + TAU / 24 * .75)), (100 + 74 * math.cos(a0 + TAU / 24), 100 + 74 * math.sin(a0 + TAU / 24))]
add("sudarshan", "chakra", "A disc with sawtooth blades and an open centre.", path(P(saw) + circ(100, 100, 44, True)) + path(circ(100, 100, 18)))
blades = ""
for j in range(6):
    lead = [rot(100 + (20 + 68 * t) * math.cos(1.1 * t), 100 + (20 + 68 * t) * math.sin(1.1 * t), j * 60) for t in [i / 30 for i in range(31)]]
    trail = [rot(100 + (20 + 68 * t) * math.cos(1.1 * t - .6 * math.sin(math.pi * t)), 100 + (20 + 68 * t) * math.sin(1.1 * t - .6 * math.sin(math.pi * t)), j * 60) for t in [i / 30 for i in range(31)]]
    blades += P(lead + trail[::-1])
add("vortex", "chakra", "Six blades swept around a still centre.", path(blades) + path(circ(100, 100, 12)))
rays = "".join(path(P(rotpts([(94, 64), (100, 22), (106, 64)], a, 100, 140))) for a in range(-80, 81, 20))
add("udaya", "chakra", "Sunrise: a half sun with rays over the horizon.",
    path(f"M40 140 A60 60 0 0 1 160 140 Z") + rays + path("M20 150 H180 V162 H20 Z"))
sun_rays = "".join(path(P(rotpts([(94, 46), (100, 12), (106, 46)], a))) for a in range(0, 360, 30))
add("surya", "chakra", "A sun disc with twelve pointed rays.", path(circ(100, 100, 46)) + sun_rays)
add("bindu ring", "chakra", "The bindu: a dot inside a single heavy ring.", path(ring(100, 100, 82, 58)) + path(circ(100, 100, 22)))

# ---------------- lotus ----------------
l3 = P(petal(100, 168, 120, 30, 0)) + P(petal(100, 168, 96, 26, -36)) + P(petal(100, 168, 96, 26, 36))
add("kamal", "lotus", "Three petals rising from one point.", path(l3, False))
l5 = "".join(P(petal(100, 170, 118 - abs(a) * .5, 22, a)) for a in (-60, -30, 0, 30, 60))
add("lotus fan", "lotus", "Five petals opening like a fan.", path(l5, False))
bud = mask("m-bud", path(P(petal(100, 186, 160, 46, 0))), '<path d="M100 40 C 96 90, 96 140, 100 184" stroke="#000" stroke-width="7" fill="none"/>')
add("bud", "lotus", "A closed lotus bud, split by one line.", bud)
rosette = "".join(P(petal(100, 100, 82, 20, a, 10)) for a in range(0, 360, 45)) + circ(100, 100, 14, True)
add("rosette", "lotus", "A lotus seen from above: eight petals.", path(rosette))
water = stroke("M24 172 Q 43 160 62 172 T 100 172 T 138 172 T 176 172", 9)
add("on water", "lotus", "A three-petal lotus on a ripple of water.",
    path(P(petal(100, 150, 104, 26, 0)) + P(petal(100, 150, 80, 22, -40)) + P(petal(100, 150, 80, 22, 40)), False) + water)
add("lotus seal", "lotus", "A lotus cut out of a solid disc.",
    mask("m-lseal", path(circ(100, 100, 88)), f'<path d="{l3}" fill="#000" transform="translate(0 -6) scale(1) "/>'))

# ---------------- diya & flame ----------------
flame = P(drop(100, 96, 22, 56))
add("diya", "light", "An oil lamp: a shallow bowl and one flame.",
    path("M36 136 Q 100 196 164 136 Q 132 150 100 150 Q 68 150 36 136 Z") + path(flame) + path("M30 128 H170 V138 H30 Z"))
add("jyoti", "light", "A flame with an inner flame cut out.", path(P(drop(100, 128, 52, 118)) + P(drop(100, 146, 24, 56))[::] .replace("M", "M", 1)))
add("deepa arch", "light", "A flame standing inside a temple arch.",
    path("M40 186 V96 Q40 30 100 18 Q160 30 160 96 V186 Z" + "M60 186 V100 Q60 50 100 40 Q140 50 140 100 V186 Z") + path(P(drop(100, 138, 18, 46))))

# ---------------- devanagari & script ----------------
add("tri", "script", "त्रि, the first syllable of tripppin, in Devanagari.", text("त्रि", 150, 152, "Rozha One"))
add("tri seal", "script", "त्रि reversed out of a solid disc.",
    mask("m-triseal", path(circ(100, 100, 90)), f'<text x="100" y="146" text-anchor="middle" font-family="Rozha One" font-size="118" fill="#000">त्रि</text>'))
add("ta", "script", "त, the letter ta, heavy and modern.", text("त", 170, 158, "Mukta", 800))
add("shirorekha", "script", "The headline bar of Devanagari with three strokes hanging from it.",
    path("M20 44 H180 V64 H20 Z") + stroke("M58 64 V150", 18, "butt") + stroke("M100 64 V128 Q100 160 130 160", 18, "butt") + stroke("M146 64 V176", 18, "butt"))
add("chandrabindu", "script", "ँ, the moon-and-dot sign: a crescent cradling a point.",
    path("M30 92 A70 70 0 0 0 170 92 A70 56 0 0 1 30 92 Z") + path(circ(100, 56, 22)))
add("visarga", "script", "ः, two stacked dots: the breath at the end of a sound.", path(circ(100, 56, 34)) + path(circ(100, 146, 34)))
add("danda", "script", "॥, the double stroke that ends a verse.", path("M58 24 H86 V176 H58 Z M114 24 H142 V176 H114 Z"))
add("anusvara", "script", "A single bindu above a resting curve.", path(circ(100, 54, 26)) + stroke("M34 126 Q100 190 166 126", 22))

# ---------------- architecture ----------------
add("jharokha", "architecture", "A pointed arch window, carved in solid.",
    path("M30 186 V100 Q30 34 100 14 Q170 34 170 100 V186 Z M56 186 V106 Q56 60 100 44 Q144 60 144 106 V186 Z"))
tiers = "".join(path(f"M{100 - w} {y} Q100 {y - 26} {100 + w} {y} V{y + 14} H{100 - w} Z") for w, y in ((70, 172), (56, 138), (42, 106), (28, 76)))
add("shikhara", "architecture", "A temple spire in stacked tiers, topped by a kalash.", tiers + path(circ(100, 52, 10)) + path("M97 30 H103 V44 H97 Z"))
steps = "".join(path(f"M{30 + i * 14} {40 + i * 26} H{170 - i * 14} V{56 + i * 26} H{30 + i * 14} Z") for i in range(5))
add("stepwell", "architecture", "A stepwell: terraces narrowing into the ground.", steps)
add("chhatri", "architecture", "A chhatri: a dome raised on slender pillars.",
    path("M40 96 A60 60 0 0 1 160 96 Z M30 96 H170 V110 H30 Z M44 110 H58 V170 H44 Z M93 110 H107 V170 H93 Z M142 110 H156 V170 H142 Z M24 170 H176 V184 H24 Z") + path(circ(100, 26, 8)))
add("stupa", "architecture", "A stupa: dome, base, and a mast of stacked umbrellas.",
    path("M36 150 A64 64 0 0 1 164 150 Z M24 150 H176 V170 H24 Z M86 78 H114 V90 H86 Z M97 30 H103 V80 H97 Z") +
    path("M78 70 Q100 62 122 70 V74 H78 Z M84 56 Q100 49 116 56 V60 H84 Z M89 43 Q100 37 111 43 V47 H89 Z"))
arches = "".join(path(f"M{x} 176 V112 Q{x} 70 {x + 22} 62 Q{x + 44} 70 {x + 44} 112 V176 Z") for x in (12, 78, 144))
add("three arches", "architecture", "Three arches side by side, like a mahal facade.", arches)

# ---------------- patterns ----------------
dots = "".join(circ(70 + 30 * (i % 3), 70 + 30 * (i // 3), 7) for i in range(9))
add("kolam", "pattern", "A kolam: a dot grid with one line looping through it.", path(dots, False) + stroke("M55 55 Q100 20 145 55 Q180 100 145 145 Q100 180 55 145 Q20 100 55 55 Z M85 85 L115 115 M115 85 L85 115", 7))
star = []
for i in range(16):
    r = 90 if i % 2 == 0 else 46
    a = -math.pi / 2 + TAU * i / 16
    star.append((100 + r * math.cos(a), 100 + r * math.sin(a)))
add("rangoli", "pattern", "An eight-point rangoli star with a dot inside.", path(P(star) + circ(100, 100, 20, True)) + path(circ(100, 100, 9)))
mand = "".join(P(petal(100, 100, 40, 10, a, 46)) for a in range(0, 360, 30)) + "".join(P(petal(100, 100, 28, 12, a + 15, 18)) for a in range(0, 360, 30))
add("mandala", "pattern", "Two rings of petals around a centre.", path(mand, False) + path(circ(100, 100, 12)))
paisley = taper([((70, 150), (40, 110), (70, 40), (120, 40)), ((120, 40), (170, 40), (176, 110), (130, 140))], 1, 1)
add("ambi", "pattern", "Paisley: a rounded drop whose tip curls back over itself.",
    path("M104 188 C 50 188 30 136 52 100 C 72 68 118 74 132 46 C 140 30 126 20 114 28 C 112 14 150 10 162 44 C 176 88 160 188 104 188 Z" + circ(96, 140, 20, True)) + path(circ(96, 140, 9)))
add("bindi", "pattern", "A bindi: one dot above a crescent.", path(circ(100, 70, 34)) + path("M44 130 A56 56 0 0 0 156 130 A56 40 0 0 1 44 130 Z"))
add("tilak", "pattern", "A tilak: a U-shape with a dot inside.", stroke("M62 30 V110 Q62 160 100 160 Q138 160 138 110 V30", 22) + path(circ(100, 92, 14)))
yan = P([(100, 24), (172, 152), (28, 152)]) + P([(100, 64), (138, 132), (62, 132)])
add("yantra", "pattern", "A triangle within a triangle, with the bindu at the centre.", path(yan) + path(circ(100, 110, 9)))

# ---------------- nature & objects ----------------
leaf = "M100 22 C 40 40 22 100 50 140 C 66 160 86 168 100 192 C 114 168 134 160 150 140 C 178 100 160 40 100 22 Z"
add("peepal", "nature", "A peepal leaf, point down, with its midrib cut out.",
    mask("m-peepal", f'<path d="{leaf}" transform="rotate(180 100 107)"/>', stroke("M100 40 L100 170", 6).replace("currentColor", "#000")))
torana_leaves = "".join(path(P(petal(x, 64, 70 if i % 2 == 0 else 52, 14, 180))) for i, x in enumerate((40, 70, 100, 130, 160)))
add("toran", "nature", "A doorway garland of mango leaves.", path("M20 56 H180 V68 H20 Z") + torana_leaves)
spiral = []
for i in range(200):
    t = i / 199 * 3.3 * math.pi
    r = 8 + 72 * (t / (3.3 * math.pi))
    spiral.append((100 + r * math.cos(t), 100 + r * math.sin(t)))
add("shankh", "nature", "A conch spiral, drawn in one heavy line.", stroke(P(spiral, False), 16))
add("kalash", "nature", "A kalash: pot, mouth, leaves and a coconut.",
    path("M54 116 Q50 182 100 186 Q150 182 146 116 Q140 96 100 96 Q60 96 54 116 Z M68 84 H132 V96 H68 Z") + path(circ(100, 52, 26)) +
    path(P(petal(100, 84, 50, 10, -62))) + path(P(petal(100, 84, 50, 10, 62))))
add("damru", "nature", "A damru drum: two triangles meeting at a waist.",
    path("M40 30 H160 L104 100 L160 170 H40 L96 100 Z") + path(circ(28, 100, 10)) + path(circ(172, 100, 10)) + stroke("M38 100 H162", 4))
json.dump([{"id": i, "name": n, "family": f, "idea": d, "body": b} for i, n, f, d, b in I], open(OUT, "w"))
print(len(I), "ideas")
