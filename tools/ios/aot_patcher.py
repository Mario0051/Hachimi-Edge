import struct
import json
import sys

def relocate_instruction(insn, from_va, to_va):
    if (insn & 0x9F000000) == 0x90000000:
        immlo = (insn >> 29) & 0x3
        immhi = (insn >> 5) & 0x7FFFF
        imm = (immhi << 2) | immlo
        if imm & 0x100000:
            imm -= 0x200000

        target_page = (from_va & ~0xFFF) + (imm << 12)
        to_page = to_va & ~0xFFF
        page_diff = (target_page - to_page) >> 12

        new_immlo = page_diff & 0x3
        new_immhi = (page_diff >> 2) & 0x7FFFF

        return [ (insn & 0x9F00001F) | (new_immlo << 29) | (new_immhi << 5) ]

    if (insn & 0x7C000000) == 0x14000000:
        imm26 = insn & 0x3FFFFFF
        if imm26 & 0x2000000:
            imm26 -= 0x4000000
        target = from_va + (imm26 * 4)
        offset = (target - to_va) // 4
        new_insn = (insn & 0xFC000000) | (offset & 0x3FFFFFF)
        return [ new_insn ]

    if (insn & 0x7E000000) == 0x34000000:
        imm19 = (insn >> 5) & 0x7FFFF
        if imm19 & 0x40000:
            imm19 -= 0x80000
        target = from_va + (imm19 * 4)
        new_cbz = (insn & 0xFF00001F) | (2 << 5)
        offset = (target - (to_va + 8)) // 4
        b_target = 0x14000000 | (offset & 0x3FFFFFF)
        return [ new_cbz, 0, b_target ]

    if (insn & 0xFF000000) == 0x54000000:
        imm19 = (insn >> 5) & 0x7FFFF
        if imm19 & 0x40000:
            imm19 -= 0x80000
        target = from_va + (imm19 * 4)
        new_bcond = (insn & 0xFF00001F) | (2 << 5)
        offset = (target - (to_va + 8)) // 4
        b_target = 0x14000000 | (offset & 0x3FFFFFF)
        return [ new_bcond, 0, b_target ]

    if (insn & 0x7E000000) == 0x36000000:
        imm14 = (insn >> 5) & 0x3FFF
        if imm14 & 0x2000:
            imm14 -= 0x4000
        target = from_va + (imm14 * 4)
        new_tbz = (insn & 0xFFF8001F) | (2 << 5)
        offset = (target - (to_va + 8)) // 4
        b_target = 0x14000000 | (offset & 0x3FFFFFF)
        return [ new_tbz, 0, b_target ]

    return [ insn ]

def generate_direct_branch(from_va, to_va):
    offset = (to_va - from_va) // 4
    if not (-0x2000000 <= offset < 0x2000000):
        raise ValueError(f"Offset {offset} out of bounds for direct branch from {hex(from_va)} to {hex(to_va)}")

    b_insn = 0x14000000 | (offset & 0x3FFFFFF)
    return struct.pack('<I', b_insn)

def generate_indirect_jump(pc_va, data_va):
    pc_page = pc_va & ~0xFFF
    data_page = data_va & ~0xFFF
    page_diff = (data_page - pc_page) >> 12

    immlo = page_diff & 3
    immhi = (page_diff >> 2) & 0x7FFFF
    adrp = 0x90000010 | (immlo << 29) | (immhi << 5)

    offset = data_va & 0xFFF
    add = 0x91000210 | (offset << 10)

    ldr = 0xF9400210
    br  = 0xD61F0200

    return struct.pack('<IIII', adrp, add, ldr, br)

def main():
    if len(sys.argv) < 4:
        print(f"Usage: {sys.argv[0]} <UnityFramework_In> <hooks.json> <UnityFramework_Out>")
        sys.exit(1)

    in_bin = sys.argv[1]
    hooks_json = sys.argv[2]
    out_bin = sys.argv[3]

    with open(hooks_json, 'r') as f:
        config = json.load(f)

    caves = config["caves"]
    text_cave = caves["text_offset"]
    data_cave_va = caves["data_va"]

    with open(in_bin, 'rb') as f:
        data = bytearray(f.read())

    hooks = config["hooks"]
    print(f"Applying {len(hooks)} AOT hooks...")

    sorted_hooks = sorted(hooks, key=lambda h: h["offset"])
    for i in range(len(sorted_hooks) - 1):
        curr_h = sorted_hooks[i]
        next_h = sorted_hooks[i + 1]
        diff = next_h["offset"] - curr_h["offset"]
        if diff < 4:
            print(f"[-] ERROR: Hook collision in {hooks_json}!")
            print(f"    '{curr_h['name']}' ({hex(curr_h['offset'])}) and '{next_h['name']}' ({hex(next_h['offset'])})")
            print(f"    are only {diff} bytes apart (< 4 bytes needed for direct jump stub)!")
            sys.exit(1)

    for idx, hook in enumerate(hooks):
        orig_off = hook["offset"]

        disp_off = text_cave + (idx * 32)
        tramp_off = disp_off + 16
        data_va   = data_cave_va + (idx * 16)

        orig_bytes = data[orig_off : orig_off + 4]
        insn = struct.unpack('<I', orig_bytes)[0]
        relocated_insns = relocate_instruction(insn, orig_off, tramp_off)

        disp_bytes = generate_indirect_jump(disp_off, data_va)

        ret_jump = generate_direct_branch(tramp_off + 4, orig_off + 4)

        if len(relocated_insns) == 1:
            tramp_bytes = struct.pack('<I', relocated_insns[0]) + ret_jump
        elif len(relocated_insns) == 3:
            tramp_bytes = struct.pack('<I', relocated_insns[0]) + ret_jump + struct.pack('<I', relocated_insns[2])
        else:
            raise ValueError("Unexpected relocated_insns length")

        data[disp_off : disp_off + 16] = disp_bytes
        data[tramp_off : tramp_off + len(tramp_bytes)] = tramp_bytes

        stub = generate_direct_branch(orig_off, disp_off)
        data[orig_off : orig_off + 4] = stub

        print(f"  [+] {hook.get('name', hex(orig_off))} -> Disp: {hex(disp_off)}, Tramp: {hex(tramp_off)}, DataVA: {hex(data_va)}")

    with open(out_bin, 'wb') as f:
        f.write(data)

    print(f"\nDone! Saved to {out_bin} (Zero bytes touched in __DATA)")

if __name__ == "__main__":
    main()