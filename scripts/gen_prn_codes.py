#!/usr/bin/env python3
"""Generate GPS L1 C/A PRN code table, verified against IS-GPS-200.

The correct approach (per IS-GPS-200):
- G1 and G2 shift registers run simultaneously for 1023 cycles
- G1 output: always from stage 10
- G2 output: XOR of TWO specific stages (satellite-dependent)
- PRN chip = G1_output XOR G2_output
"""

CODE_LENGTH = 1023

# G2 output tap pairs for each PRN (1-indexed stages)
# Per IS-GPS-200 Table 3-I: Code Phase Assignments
SV_TAPS = {
    1: [2,6],   2: [3,7],   3: [4,8],   4: [5,9],
    5: [1,9],   6: [2,10],  7: [1,8],   8: [2,9],
    9: [3,10],  10: [2,3],  11: [3,4],  12: [5,6],
    13: [6,7],  14: [7,8],  15: [8,9],  16: [9,10],
    17: [1,4],  18: [2,5],  19: [3,6],  20: [4,7],
    21: [5,8],  22: [6,9],  23: [1,3],  24: [4,6],
    25: [5,7],  26: [6,8],  27: [7,9],  28: [8,10],
    29: [1,6],  30: [2,7],  31: [3,8],  32: [4,9],
}

def prn_code(sv):
    """Generate full 1023-chip PRN code for satellite sv (1-32)."""
    taps = SV_TAPS[sv]
    # G1: feedback from stages 3,10 (1-indexed) → indices 2,9
    # G2: feedback from stages 2,3,6,8,9,10 → indices 1,2,5,7,8,9
    g1 = [1]*10
    g2 = [1]*10
    code = []
    for _ in range(CODE_LENGTH):
        g1_out = g1[9]
        g2_out = (g2[taps[0]-1] + g2[taps[1]-1]) % 2
        code.append((g1_out + g2_out) % 2)
        # Feedback
        g1_fb = (g1[2] + g1[9]) % 2
        g2_fb = sum(g2[i] for i in [1,2,5,7,8,9]) % 2
        # Shift right
        g1 = [g1_fb] + g1[:9]
        g2 = [g2_fb] + g2[:9]
    return code

# Verify PRN 1 first 10 chips (octal 1440): 1 1 0 0 1 0 0 0 0 0
prn1 = prn_code(1)
expected = [1, 1, 0, 0, 1, 0, 0, 0, 0, 0]
assert prn1[:10] == expected, f"PRN1 mismatch: {prn1[:10]} != {expected}"
print("PRN1 verification PASSED")

# Verify balance: each code has 512 ones, 511 zeros (Golden code property)
for sv in range(1, 33):
    code = prn_code(sv)
    ones = sum(code)
    assert ones == 512, f"PRN {sv}: expected 512 ones, got {ones}"
print("All 32 codes balance verified (512 ones each)")

# Generate Rust include file
with open("crates/gnss-data/src/prn_codes.inl", "w") as f:
    f.write("// Auto-generated GPS L1 C/A PRN code table. DO NOT EDIT.\n")
    f.write("// Verified against IS-GPS-200: PRN1 first 10 chips match.\n\n")
    f.write("[\n")
    for sv in range(1, 33):
        code = prn_code(sv)
        f.write(f"    // PRN {sv}\n")
        f.write("    [\n")
        for i in range(0, CODE_LENGTH, 16):
            chunk = code[i:i+16]
            vals = ", ".join(" 1" if v else "-1" for v in chunk)
            f.write(f"        {vals},\n")
        f.write("    ],\n")
    f.write("]\n")

print(f"\nGenerated crates/gnss-data/src/prn_codes.inl")
print(f"Total: 32 PRN codes × {CODE_LENGTH} chips")
