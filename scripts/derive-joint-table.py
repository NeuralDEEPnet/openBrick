#!/usr/bin/env python3
"""Derive standards/joints-v1.json (the clip, hinge, ball and bar joint table) from the user's own
copy of the LDraw Parts Library: python3 -I scripts/derive-joint-table.py <ldraw dir> <out.json>

What it reads is each joint primitive's own geometry (the matrices of the cylinders, cones and
spheres it is made of), never another program's connection data (LDCad's shadow library is not
read). The result is a table of facts about those primitives (axis and a point on it, in the
primitive's own frame, integer LDU), kept as data so the importer, the schema and the reviewers
can read it. The library is CC BY 2.0 / CC BY 4.0: the table carries the library's credit.
Standard numbers (bar radius 4, ball radius 8) are read from standards/dimensions-v1.json.
"""
import json, math, os, re, sys

root, out = sys.argv[1], sys.argv[2]
STD = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'standards', 'dimensions-v1.json')))
feat = {f['id']: f for f in STD['features']}
BAR_R = feat['bar_diameter']['ldraw_ldu'] / 2           # 3.975 -> LDraw models bars at r = 4
BALL_R = 8.0                                            # standard row ball_small (14.75 LDU) is modelled at r = 8 by LDraw

def find(n):
    n = n.lower().replace('\\', '/')
    for d in ('p/', 'parts/s/', 'parts/', 'p/48/'):
        f = os.path.join(root, d + n)
        if os.path.exists(f):
            return f
    return None

def mm(a, b): return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
def mv(M, T, p): return [T[i] + sum(M[i][k] * p[k] for k in range(3)) for i in range(3)]

def instances(name):
    """Every cylinder, cone, sphere primitive instance under a primitive: (primitive, origin, axis, radius, length)."""
    res = []
    def walk(n, M, T, depth):
        f = find(n)
        if not f or depth > 8:
            return
        for l in open(f, errors='replace'):
            t = l.split()
            if len(t) >= 15 and t[0] == '1':
                v = list(map(float, t[2:14]))
                m = [v[3:6], v[6:9], v[9:12]]
                nt, nm = mv(M, T, v[0:3]), mm(M, m)
                b = t[14].lower().replace('\\', '/').split('/')[-1]
                kind = None
                if re.match(r'^\d+-\d+cyl[ioc]\.dat$', b): kind = 'cyl'
                elif re.match(r'^\d+-\d+con\d+\.dat$', b): kind = 'cone'
                elif re.match(r'^\d+-\d+sphe\.dat$', b): kind = 'sphere'
                if kind:
                    ax = [nm[i][1] for i in range(3)]
                    ln = math.sqrt(sum(a * a for a in ax))
                    rad = math.sqrt(sum(nm[i][0] ** 2 for i in range(3)))
                    res.append((kind, b, nt, ax, rad, ln))
                else:
                    walk(t[14], nm, nt, depth + 1)
    walk(name + '.dat', [[1, 0, 0], [0, 1, 0], [0, 0, 1]], [0, 0, 0], 0)
    return res

def lattice(ax):
    ln = math.sqrt(sum(a * a for a in ax))
    if ln < 1e-9: return None
    u = [a / ln for a in ax]
    for k in range(3):
        if abs(abs(u[k]) - 1) < 1e-3:
            r = [0, 0, 0]; r[k] = 1 if u[k] > 0 else -1   # an axis has no sign: keep the first non-zero positive
            r[k] = 1
            return r
    return None

def desc(name):
    f = find(name + '.dat')
    return open(f, errors='replace').readline()[2:].strip() if f else ''

def near_origin(point, axis):
    """The point of the axis line nearest the primitive's origin (along-axis coordinate 0)."""
    k = axis.index(1)
    p = list(point); p[k] = 0.0
    return p

def r1(x):
    """A coordinate: a whole number of LDU when it is within 0.05 of one, else to 2 places."""
    r = round(x)
    return float(r) + 0.0 if abs(x - r) <= 0.05 else round(x + 0.0, 2)


def sphere_vertices(name, radius=BALL_R):
    """Vertices of a primitive on the sphere of `radius` about its origin, and all vertices."""
    vs = []
    def walk(n, M, T, depth):
        f = find(n)
        if not f or depth > 8:
            return
        for l in open(f, errors='replace'):
            t = l.split()
            if len(t) >= 15 and t[0] == '1':
                v = list(map(float, t[2:14]))
                walk(t[14], mm(M, [v[3:6], v[6:9], v[9:12]]), mv(M, T, v[0:3]), depth + 1)
            elif t and t[0] in '34':
                for i in range(3 if t[0] == '3' else 4):
                    vs.append(mv(M, T, list(map(float, t[2 + 3 * i:5 + 3 * i]))))
    walk(name + '.dat', [[1, 0, 0], [0, 1, 0], [0, 0, 1]], [0, 0, 0], 0)
    on = [v for v in vs if abs(math.sqrt(sum(x * x for x in v)) - radius) < 0.3]
    return len(on), len(vs)

entries, skipped = [], []
names = sorted({os.path.basename(f)[:-4] for d in ('p',) for f in os.listdir(os.path.join(root, d))
                if re.match(r'^(clip\d+|clh[\w-]*|arm[123]|h[12]|phandle\d|joint8ball|joint8socket\d)\.dat$', f)})
for n in names:
    d = desc(n)
    inst = instances(n)
    e = {'primitive': n, 'description': d}
    if re.match(r'^clip\d+$', n):
        bores = [i for i in inst if i[0] == 'cyl' and abs(i[4] - BAR_R) < 0.15 and i[5] >= 6 and lattice(i[3])]
        if not bores:
            skipped.append((n, 'no cylinder of the bar radius')); continue
        k, b, t, ax, rad, ln = bores[0]
        a = lattice(ax)
        mid = [t[i] + ax[i] / 2 for i in range(3)]
        e.update(kind='clipF', axis=a, origin=[r1(x) for x in mid], method='bore-cylinder',
                 evidence=f'{b}: a cylinder of radius {r1(rad)} (the bar radius, bar_diameter / 2) and length {r1(ln)}; origin is its middle')
    elif n in ('joint8ball',):
        sph = [i for i in inst if i[0] == 'sphere' and abs(i[4] - BALL_R) < 0.15]
        stem = [i for i in inst if i[0] == 'cyl' and abs(i[4] - 4) < 0.15 and lattice(i[3])]
        if not sph or not stem:
            skipped.append((n, 'no ball or no stem')); continue
        e.update(kind='ballM', axis=lattice(stem[0][3]), origin=[r1(x) for x in sph[0][2]], method='sphere-centre',
                 evidence=f'{sph[0][1]}: a sphere of radius {r1(sph[0][4])}, centred at the origin of the instance; the stem is a cylinder of radius 4 along the axis')
    elif n.startswith('joint8socket'):
        # The socket is drawn around the centre of the ball it holds, which is the origin
        # (joint8ball.dat has its sphere there): check that a good share of its vertices lie on the
        # sphere of the ball's radius about the origin. A ball mates by position, so the axis is
        # only the direction the socket opens; it is not used to mate.
        on, total = sphere_vertices(n)
        if on < 100:
            skipped.append((n, f'only {on} of {total} vertices on the ball sphere about the origin')); continue
        e.update(kind='socketF', axis=[0, 0, 1], origin=[0.0, 0.0, 0.0], method='concentric-with-ball',
                 evidence=f'{on} of its {total} vertices lie on the sphere of radius {r1(BALL_R)} about the origin, the sphere of joint8ball.dat; the axis is not used to mate a ball')
    elif n.startswith('clh') or n in ('arm1', 'arm2', 'arm3'):
        if n in ('clh-base', 'clh-finger', 'clh2c', 'clh3q', 'clh5s', 'clh6c', 'clh6d', 'clh6u'):
            skipped.append((n, 'a sub-primitive of the click hinge, not a part feature')); continue
        low = d.lower()
        if 'single finger' in low or '3 finger' in low: parity = 'hingeO'
        elif 'dual finger' in low or '2 finger' in low: parity = 'hingeE'
        else:
            skipped.append((n, 'finger count not in the description')); continue
        cones = [i for i in inst if i[0] == 'cone' and lattice(i[3])]
        cyls = [i for i in inst if i[0] == 'cyl' and i[4] >= 3 and lattice(i[3])]
        src = cones[0] if cones else (cyls[0] if cyls else None)
        if not src:
            skipped.append((n, 'no cone or cylinder on a lattice axis')); continue
        a = lattice(src[3])
        e.update(kind=parity, axis=a, origin=[r1(x) for x in near_origin(src[2], a)], method='hinge-pin',
                 evidence=f'{src[1]}: instance at {[r1(x) for x in src[2]]} with its axis along {a}; origin is the point of that axis line nearest the primitive origin; {"odd" if parity == "hingeO" else "even"} finger count from the description "{d}"')
    else:
        skipped.append((n, 'no rule (a hinge plate or bar handle: not calibrated)')); continue
    entries.append(e)

doc = {
    'openbrick': '1.0', 'kind': 'joints', 'version': '1.0.0', 'license': 'CC-BY-4.0',
    'credit': {'author': 'OpenBrick3D contributors, derived from the LDraw Parts Library (LDraw.org, CC BY 2.0 / CC BY 4.0)', 'url': 'https://www.ldraw.org'},
    'library': 'LDraw Parts Library, update 2026-10 (complete.zip, SHA-256 023083918da2d3697c794fb4267975509584f3611b993095874206a803fa610f)',
    'notes': 'Joint primitives of the LDraw Parts Library, each with the axis of the joint and a point on that axis in the primitive\'s own frame (integer LDU or halves), derived from the primitive\'s own cylinders, cones and spheres by scripts/derive-joint-table.py. A clip holds a bar of radius 4 LDU (standard row bar_diameter); the ball is radius 8. Hinge halves are told apart by the parity of their finger count: a hinge with an odd number of fingers mates one with an even number. Primitives that are not listed are not modelled.',
    # Bars have no primitive of their own (Q04): the importer reads a cylinder of the bar radius in a part
    # whose description names a bar or a handle (a clip grips it). These are the parameters of that rule.
    'bars': {
        'radius': r1(BAR_R) if abs(BAR_R - round(BAR_R)) <= 0.05 else 4.0,
        'radiusTolerance': 0.15,
        'minLength': 6.0,
        'descriptionWords': ['bar', 'handle'],
        'source': 'standards/dimensions-v1.json bar_diameter (7.95 LDU, which LDraw models as a radius of 4); the clip bores of this table are cylinders of the same radius',
    },
    'primitives': entries,
}
# One line per primitive, like the other data files.
with open(out, 'w') as f:
    f.write('{\n')
    top = [k for k in doc if k != 'primitives']
    for k in top:
        f.write(f' {json.dumps(k)}: {json.dumps(doc[k])},\n')
    f.write(' "primitives": [\n')
    f.write(',\n'.join('  ' + json.dumps(e) for e in doc['primitives']))
    f.write('\n ]\n}\n')
print(f'{len(entries)} primitives in the table, {len(skipped)} left out')
for n, why in skipped: print('  left out', n, '-', why)
