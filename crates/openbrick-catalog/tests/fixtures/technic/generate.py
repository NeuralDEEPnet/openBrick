#!/usr/bin/env python3
"""Regenerates the synthetic mini Technic library next to this file (CC0, written for OpenBrick3D tests; nothing here is copied from the LDraw library):
    python3 -I generate.py <this directory> ../ldraw/ldraw
The primitive and part names are the LDraw ones the importer and the joint table recognise; the shapes are our own boxes and prisms."""
import math, os, sys, shutil
out = sys.argv[1]
src = sys.argv[2]   # tests/fixtures/ldraw/ldraw (the older synthetic library), for cube/stud
os.makedirs(out + '/ldraw/p', exist_ok=True)
os.makedirs(out + '/ldraw/parts', exist_ok=True)
HDR = lambda desc, name, org, extra='': (
    f"0 {desc}\n0 Name: {name}\n0 Author: OpenBrick3D tests\n0 !LDRAW_ORG {org} UPDATE 2026-01\n"
    "0 !LICENSE Synthetic test file written for OpenBrick3D, CC0\n" + extra + "\n0 BFC CERTIFY CCW\n\n")
def w(path, text):
    open(out + '/ldraw/' + path, 'w').write(text)
for f in ['cube.dat', 'stud.dat']:
    shutil.copy(src + '/p/' + f, out + '/ldraw/p/' + f)
for f in ['CAreadme.txt', 'CAlicense.txt']:
    shutil.copy(src + '/' + f, out + '/ldraw/' + f)

def fmt(v):
    s = ('%.4f' % v).rstrip('0').rstrip('.')
    return s if s not in ('-0', '') else '0'
def tri(a, b, c, hint):
    # orient (b-a)x(c-a) along the outward hint
    u = [b[i]-a[i] for i in range(3)]; v = [c[i]-a[i] for i in range(3)]
    n = [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]]
    if sum(n[i]*hint[i] for i in range(3)) < 0:
        b, c = c, b
    return '3 16 ' + ' '.join(fmt(x) for p in (a, b, c) for x in p) + '\n'

# --- connect.dat: a 16-sided pin shaft of radius 6, 20 long along -Y, with a 8-radius collar disc 1 thick at the base
def prism_y(r, y0, y1, n=16):
    t = ''
    ring = [(r*math.cos(2*math.pi*k/n), r*math.sin(2*math.pi*k/n)) for k in range(n)]
    for k in range(n):
        a, b = ring[k], ring[(k+1) % n]
        mid = ((a[0]+b[0])/2, (a[1]+b[1])/2)
        h = (mid[0], 0, mid[1])
        p = lambda q, y: (q[0], y, q[1])
        t += tri(p(a, y0), p(b, y0), p(b, y1), h) + tri(p(a, y0), p(b, y1), p(a, y1), h)
        t += tri((0, y0, 0), p(a, y0), p(b, y0), (0, -1 if y0 < y1 else 1, 0)) if False else ''
        top = (0, 1 if y1 > y0 else -1, 0)
        t += tri((0, y1, 0), p(a, y1), p(b, y1), top) + tri((0, y0, 0), p(a, y0), p(b, y0), (0, -top[1], 0))
    return t
w('p/connect.dat', HDR('Synthetic Pin 1.0 with Base Collar', 'connect.dat', 'Primitive') + prism_y(6, 0, -20))
os.makedirs(out + '/ldraw/p/8', exist_ok=True)
os.makedirs(out + '/ldraw/p/48', exist_ok=True)
w('p/8/connect.dat', HDR('Synthetic Pin 1.0 with Base Collar', 'connect.dat', 'Primitive') + prism_y(6, 0, -20, 8))
w('p/48/connect.dat', HDR('Synthetic Pin 1.0 with Base Collar', 'connect.dat', 'Primitive') + prism_y(6, 0, -20, 48))

# --- axlehol8.dat: the cross of an axle, 1 long along +Y, from five boxes (scaled by the part)
def box(x0, x1, y0, y1, z0, z1):
    cx, cz = (x0+x1)/2, (z0+z1)/2
    return f"1 16 {fmt(cx)} {fmt(y0)} {fmt(cz)} {fmt((x1-x0)/2)} 0 0 0 {fmt(y1-y0)} 0 0 0 {fmt((z1-z0)/2)} cube.dat\n"
cross = lambda a, s, y0, y1: (box(-s, -a, y0, y1, -a, a) + box(a, s, y0, y1, -a, a) +
                              box(-a, a, y0, y1, -s, -a) + box(-a, a, y0, y1, a, s) + box(-a, a, y0, y1, -a, a))
w('p/axlehol8.dat', HDR('Synthetic Axle Perimeter', 'axlehol8.dat', 'Primitive') + cross(2.0, 6.0, 0, 1))
w('p/beamhole.dat', HDR('Synthetic Beam Hole 1.0', 'beamhole.dat', 'Primitive') + '3 16 0 0 0 1 0 0 0 0 1\n')
w('p/axlehole.dat', HDR('Synthetic Axle Hole', 'axlehole.dat', 'Primitive') + '3 16 0 0 0 1 0 0 0 0 1\n')

# --- tcellp.dat: a 20 x 24 x 20 block (x, y, z) with a 16-sided round hole of radius 8 along X at y = 12, z = 0
def hole_cell():
    n = 16; r = 8.0; cy, cz = 12.0, 0.0
    X0, X1 = -10.0, 10.0
    Y0, Y1, Z0, Z1 = 0.0, 24.0, -10.0, 10.0
    inner = [(cy + r*math.sin(2*math.pi*k/n), cz + r*math.cos(2*math.pi*k/n)) for k in range(n)]   # (y, z)
    th = [2*math.pi*k/n for k in range(n)]                                                      # angle from +z toward +y
    def ray(t):
        dy, dz = math.sin(t), math.cos(t)
        best = 1e9
        for lim, d, c in ((Y0, dy, cy), (Y1, dy, cy), (Z0, dz, cz), (Z1, dz, cz)):
            if abs(d) > 1e-9:
                s = (lim - c) / d
                if s > 0:
                    best = min(best, s)
        return (cy + best*dy, cz + best*dz)
    corners = [(Y0, Z0), (Y0, Z1), (Y1, Z0), (Y1, Z1)]
    ang = lambda p: math.atan2(p[0]-cy, p[1]-cz) % (2*math.pi)
    outer = []   # boundary points ccw by angle
    cells = []   # per sector: list of outer points, ascending angle
    for k in range(n):
        a0, a1 = th[k], th[(k+1) % n] if k+1 < n else 2*math.pi
        pts = [ray(a0)]
        for c in sorted(corners, key=ang):
            if a0 + 1e-9 < ang(c) < a1 - 1e-9:
                pts.append(c)
        pts.append(ray(a1 if k+1 < n else 0.0))
        cells.append(pts)
        outer.extend(pts[:-1])
    t = ''
    P = lambda x, q: (x, q[0], q[1])
    for k in range(n):
        i0, i1 = inner[k], inner[(k+1) % n]
        pts = cells[k]
        for x, nx in ((X0, -1), (X1, 1)):
            poly = [i0, i1] + list(reversed(pts))   # a convex cell: inner chord then outer boundary back
            for m in range(1, len(poly)-1):
                t += tri(P(x, poly[0]), P(x, poly[m]), P(x, poly[m+1]), (nx, 0, 0))
        # inner wall: faces toward the axis
        mid = ((i0[0]+i1[0])/2, (i0[1]+i1[1])/2)
        h = (0, cy - mid[0], cz - mid[1])
        t += tri(P(X0, i0), P(X1, i0), P(X1, i1), h) + tri(P(X0, i0), P(X1, i1), P(X0, i1), h)
    # outer walls along the boundary points
    for k in range(len(outer)):
        a, b = outer[k], outer[(k+1) % len(outer)]
        mid = ((a[0]+b[0])/2, (a[1]+b[1])/2)
        h = (0, mid[0]-cy, mid[1]-cz)
        t += tri(P(X0, a), P(X1, a), P(X1, b), h) + tri(P(X0, a), P(X1, b), P(X0, b), h)
    return t
w('p/tcellp.dat', HDR('Synthetic Technic Cell with Round Hole', 'tcellp.dat', 'Primitive') + hole_cell())

# --- tcella.dat: the same block with a cross hole (arm half-width 2.4, span 6.2) along X, from boxes (all along X: x -10..10)
def axle_cell():
    a, s = 2.4, 6.2
    cy = 12.0
    t = ''
    def bx(y0, y1, z0, z1):
        return box(-10, 10, y0, y1, z0, z1).replace('cube.dat', 'cube.dat')
    # strips in y (this frame has X as the long axis, y up/down, z across): cross in (y, z)
    ys = [0, cy - s, cy - a, cy + a, cy + s, 24]
    t += bx(0, cy - s, -10, 10)
    t += bx(cy - s, cy - a, -10, -a) + bx(cy - s, cy - a, a, 10)
    t += bx(cy - a, cy + a, -10, -s) + bx(cy - a, cy + a, s, 10)
    t += bx(cy + a, cy + s, -10, -a) + bx(cy + a, cy + s, a, 10)
    t += bx(cy + s, 24, -10, 10)
    return t
w('p/tcella.dat', HDR('Synthetic Technic Cell with Cross Hole', 'tcella.dat', 'Primitive') + axle_cell())

# --- parts
X_AXIS = '0 1 0 -1 0 0 0 0 1'     # maps local +Y onto world +X ... (a b c d e f g h i row-major as LDraw writes them)
def beamhole(x, y, z):
    # the connector axis is local +Y; turn it onto X: (x', y', z') = (y, -x, z) -> matrix rows 0 1 0 / -1 0 0 / 0 0 1
    return f"1 16 {x} {y} {z} 0 1 0 -1 0 0 0 0 1 beamhole.dat\n"
def part(num, desc, body, cat='Technic'):
    return HDR(desc, f'{num}.dat', 'Part', f"0 !CATEGORY {cat}\n")
# 3701: Technic brick 1 x 4 with holes: three round-hole cells along z at -20, 0, 20 and two end blocks, four studs
studs4 = ''.join(f"1 16 0 0 {z} 1 0 0 0 1 0 0 0 1 stud.dat\n" for z in (-30, -10, 10, 30))
cells = ''.join(f"1 16 0 0 {z} 1 0 0 0 1 0 0 0 1 tcellp.dat\n" for z in (-20, 0, 20))
ends = box(-10, 10, 0, 24, -40, -30) + box(-10, 10, 0, 24, 30, 40)
w('parts/3701.dat', part('3701', 'Technic Brick  1 x  4 with Holes', '') + cells + ends + studs4 +
  ''.join(beamhole(0, 12, z) for z in (-20, 0, 20)))
# 3704: Technic brick 1 x 2 with axle hole: cross-hole cell at z = 0 (20 wide: z -10..10), end blocks, two studs
w('parts/3704.dat', part('3704', 'Technic Brick  1 x  2 with Axle Hole', '') +
  "1 16 0 0 0 1 0 0 0 1 0 0 0 1 tcella.dat\n" + box(-10, 10, 0, 24, -20, -10) + box(-10, 10, 0, 24, 10, 20) +
  ''.join(f"1 16 0 0 {z} 1 0 0 0 1 0 0 0 1 stud.dat\n" for z in (-10, 10)) +
  "1 16 -10 12 0 0 20 0 -1 0 0 0 0 1 axlehole.dat\n")
# 3673: Technic pin, two connect.dat halves (each 20 long) from the middle outwards along +X and -X
w('parts/3673.dat', part('3673', 'Technic Pin', '') +
  "1 16 0 0 0 0 1 0 0 0 -1 -1 0 0 connect.dat\n1 16 0 0 0 0 -1 0 0 0 -1 1 0 0 connect.dat\n")
# 3705: Technic axle 4: 80 long along X (the cross prism scaled to 80 along its +Y, turned onto X, centred)
w('parts/3705.dat', part('3705', 'Technic Axle  4', '') + "1 16 -40 0 0 0 80 0 -1 0 0 0 0 1 axlehol8.dat\n".replace("0 80 0 -1 0 0 0 0 1", "0 80 0 -1 0 0 0 0 1"))
# 3004 / 3005: plain bricks (hollow, as the older fixture's) so a Technic brick can sit on a plain one
src005 = open(src + '/parts/3005.dat').read().replace('Brick  1 x  1', 'Brick  1 x  1')
w('parts/3005.dat', src005)

# --- joints: a clip, a bar, a ball with its socket, two hinge halves (all synthetic shapes; the primitive names are the
# LDraw ones the joint table (standards/joints-v1.json) lists, so the importer reads the same table it reads for the library)
w('p/4-4cyli.dat', HDR('Synthetic Cylinder 1.0', '4-4cyli.dat', 'Primitive') + prism_y(1, 0, 1))
# clip3: bore along X (the joint table: axis +X through (y 2, z 0)), a ring of boxes around a void 9 x 9 and a neck on the +z side
w('p/clip3.dat', HDR('Synthetic Clip Horizontal with Parallel Sides', 'clip3.dat', 'Primitive') +
  box(-4, 4, -5.5, -2.5, -7.5, 7.5) + box(-4, 4, 6.5, 9.5, -7.5, 7.5) +
  box(-4, 4, -2.5, 6.5, -7.5, -4.5) + box(-4, 4, -2.5, 6.5, 4.5, 7.5) + box(-3, 3, -2, 6, 7.5, 10))
plate = box(-10, 10, 0, 8, -10, 10)
stud1 = "1 16 0 0 0 1 0 0 0 1 0 0 0 1 stud.dat\n"
# the clip's bore at (0, -2, -20): the primitive's own point (0, 2, 0) placed by an instance at (0, -4, -20)
w('parts/4085.dat', HDR('Plate  1 x  1 with Clip Horizontal', '4085.dat', 'Part', '0 !CATEGORY Plate\n') + plate + stud1 +
  "1 16 0 -4 -20 1 0 0 0 1 0 0 0 1 clip3.dat\n")
w('parts/87994.dat', HDR('Bar  3L', '87994.dat', 'Part', '0 !CATEGORY Bar\n') +
  "1 16 -30 0 0 0 60 0 4 0 0 0 0 4 4-4cyli.dat\n")
w('p/joint8ball.dat', HDR('Synthetic Joint-8 Ball with Straight Bar', 'joint8ball.dat', 'Primitive') +
  box(-6, 6, -6, 6, -6, 6) + box(-3, 3, -3, 3, 6, 20))
w('p/joint8socket2.dat', HDR('Synthetic Joint-8 Socket Free Square', 'joint8socket2.dat', 'Primitive') +
  box(-10, -7, -10, 10, -10, 10) + box(7, 10, -10, 10, -10, 10) + box(-7, 7, -10, -7, -10, 10) +
  box(-7, 7, 7, 10, -10, 10) + box(-7, 7, -7, 7, -10, -7))
w('parts/22890.dat', HDR('Technic Ball Joint', '22890.dat', 'Part', '0 !CATEGORY Technic\n') +
  "1 16 0 0 0 1 0 0 0 1 0 0 0 1 joint8ball.dat\n")
w('parts/14418.dat', HDR('Technic Socket Joint', '14418.dat', 'Part', '0 !CATEGORY Technic\n') +
  "1 16 0 0 0 1 0 0 0 1 0 0 0 1 joint8socket2.dat\n")
# clh4: a "half dual finger": two fingers along X around the hinge axis (X through y 0, z 4); clh1: a single finger around (y 0, z -10)
w('p/clh4.dat', HDR('Synthetic Click Lock Hinge Half Dual Finger', 'clh4.dat', 'Primitive') +
  box(-9, -4, -4, 4, 0, 8) + box(4, 9, -4, 4, 0, 8))
w('p/clh1.dat', HDR('Synthetic Click Lock Hinge Single Finger for Bricks', 'clh1.dat', 'Primitive') +
  box(-3, 3, -4, 4, -14, -6))
# 9101 (two fingers, hingeE) with its axis at z = -20 and 9102 (one finger, hingeO), both plates standing on the ground
w('parts/9101.dat', HDR('Hinge Plate  1 x  1 with 2 Fingers', '9101.dat', 'Part', '0 !CATEGORY Hinge\n') + plate + stud1 +
  "1 16 0 4 -24 1 0 0 0 1 0 0 0 1 clh4.dat\n")
w('parts/9102.dat', HDR('Hinge Plate  1 x  1 with 1 Finger', '9102.dat', 'Part', '0 !CATEGORY Hinge\n') + plate + stud1 +
  "1 16 0 4 -10 1 0 0 0 1 0 0 0 1 clh1.dat\n")
# a clip primitive the joint table does not calibrate (clip4): the part is flagged, its stud still snaps
w('p/clip4.dat', HDR('Synthetic Clip for Roadsign and Click-Hinge', 'clip4.dat', 'Primitive') + '3 16 0 0 0 1 0 0 0 0 1\n')
w('parts/9103.dat', HDR('Plate  1 x  1 with Clip Roadsign', '9103.dat', 'Part', '0 !CATEGORY Plate\n') + plate + stud1 +
  "1 16 0 4 -10 1 0 0 0 1 0 0 0 1 clip4.dat\n")
print('ok')
