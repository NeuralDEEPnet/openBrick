# Benchmarks

`devices/` holds the device render harness CSV (laptop and phone results for 10k and 100k instances at each LOD); contributors add rows for their hardware. `harness/` is the page that measures them; [devices/README.md](devices/README.md) explains how to run it, including on a phone. Criterion benches for snap, collision and evaluate land in M1 and M3a; CI fails on a 20 percent regression.
