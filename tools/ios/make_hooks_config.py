import struct
import json
import re
import sys
import os

TARGET_OS = "ios"

def split_cfg_args(s):
    args = []
    cur = []
    depth = 0
    for ch in s:
        if ch == '(':
            depth += 1
        elif ch == ')':
            depth -= 1
        elif ch == ',' and depth == 0:
            args.append("".join(cur).strip())
            cur = []
            continue
        cur.append(ch)
    if cur:
        args.append("".join(cur).strip())
    return args

def eval_cfg(expr, target_os=TARGET_OS):
    expr = expr.strip()
    if expr.startswith("target_os"):
        m = re.search(r'target_os\s*=\s*["\']([^"\']+)["\']', expr)
        return m is not None and m.group(1) == target_os
    if expr.startswith("not(") and expr.endswith(")"):
        return not eval_cfg(expr[4:-1], target_os)
    if expr.startswith("any(") and expr.endswith(")"):
        return any(eval_cfg(a, target_os) for a in split_cfg_args(expr[4:-1]))
    if expr.startswith("all(") and expr.endswith(")"):
        return all(eval_cfg(a, target_os) for a in split_cfg_args(expr[4:-1]))
    return True

def strip_inactive_cfg(text, target_os=TARGET_OS):
    result = []
    i = 0
    n = len(text)
    while i < n:
        if text.startswith("#[cfg(", i):
            depth = 1
            j = i + 6
            while j < n and depth > 0:
                if text[j] == '(': depth += 1
                elif text[j] == ')': depth -= 1
                j += 1
            if j < n and text[j] == ']':
                j += 1
                cfg_expr = text[i+6 : j-2]
                is_active = eval_cfg(cfg_expr, target_os)

                k = j
                while k < n and text[k].isspace():
                    k += 1

                if not is_active:
                    if k < n and text[k] == '{':
                        brace_depth = 1
                        m = k + 1
                        while m < n and brace_depth > 0:
                            if text[m] == '{': brace_depth += 1
                            elif text[m] == '}': brace_depth -= 1
                            m += 1
                        i = m
                        continue
                    else:
                        semi = text.find(';', k)
                        brace = text.find('{', k)
                        if semi != -1 and (brace == -1 or semi < brace):
                            i = semi + 1
                            continue
                        elif brace != -1:
                            brace_depth = 1
                            m = brace + 1
                            while m < n and brace_depth > 0:
                                if text[m] == '{': brace_depth += 1
                                elif text[m] == '}': brace_depth -= 1
                                m += 1
                            i = m
                            continue
                else:
                    i = j
                    continue
        result.append(text[i])
        i += 1
    return "".join(result)

def extract_patterns_and_icalls_from_rust(hook_root="src/il2cpp/hook"):
    managed_patterns = set()
    icall_signatures = set()

    class_def_re = re.compile(
        r'get_class_or_return!\s*\([^,]+,\s*["\']?([^,"\']*)["\']?,\s*(\w+)\s*\)'
    )
    nested_def_re = re.compile(
        r'find_nested_class_or_return!\s*\(\s*(\w+)\s*,\s*(\w+)\s*\)'
    )
    method_bind_re = re.compile(
        r'let\s+(\w+)\s*=\s*get_method(?:_overload)?_addr\s*\(\s*(\w+)\s*,\s*[c]?"([^"]+)"',
        re.MULTILINE
    )
    icall_bind_re = re.compile(
        r'let\s+(\w+)\s*=\s*il2cpp_resolve_icall\s*\(\s*c"([^"]+)"',
        re.MULTILINE
    )
    new_hook_re = re.compile(r'new_hook!\s*\(\s*(\w+)\s*,')
    mod_decl_re = re.compile(r'(?:pub\s+)?mod\s+(\w+)\s*;')

    folder_parents = {}
    disabled_paths = set()

    for root, _, files in os.walk(hook_root):
        if "mod.rs" in files:
            mod_path = os.path.join(root, "mod.rs")
            with open(mod_path, "r", encoding="utf-8") as f:
                raw_mod = f.read()

            all_mods = set(mod_decl_re.findall(raw_mod))
            active_content = strip_inactive_cfg(raw_mod, TARGET_OS)
            active_mods = set(mod_decl_re.findall(active_content))

            for inactive in (all_mods - active_mods):
                disabled_paths.add(os.path.normpath(os.path.join(root, inactive)))

            for ns, cls in class_def_re.findall(active_content):
                ns_clean = f"{ns}." if ns and ns != '""' else ""
                folder_parents[cls] = f"{ns_clean}{cls}"

    for root, _, files in os.walk(hook_root):
        if os.path.normpath(root) in disabled_paths:
            continue

        for file in files:
            if not file.endswith(".rs"):
                continue

            module_name = file[:-3]
            file_base_path = os.path.normpath(os.path.join(root, module_name))

            if file_base_path in disabled_paths:
                continue

            file_path = os.path.join(root, file)
            with open(file_path, "r", encoding="utf-8") as f:
                raw_content = f.read()

            content = strip_inactive_cfg(raw_content, TARGET_OS)

            hooked_vars = set(new_hook_re.findall(content))
            if not hooked_vars:
                continue

            file_classes = {}

            for ns, cls in class_def_re.findall(content):
                ns_clean = f"{ns}." if ns and ns != '""' else ""
                file_classes[cls] = f"{ns_clean}{cls}"

            for parent, nested in nested_def_re.findall(content):
                parent_full = file_classes.get(parent) or folder_parents.get(parent, parent)
                file_classes[nested] = f"{parent_full}.{nested}"

            for var_name, cls_name, method_name in method_bind_re.findall(content):
                if var_name in hooked_vars or any(method_name.lower() in hv.lower() for hv in hooked_vars):
                    full_cls = file_classes.get(cls_name) or folder_parents.get(cls_name, cls_name)
                    escaped_cls = re.escape(full_cls)
                    managed_patterns.add(rf"^{escaped_cls}\$\${re.escape(method_name)}$")

            for var_name, full_sig in icall_bind_re.findall(content):
                if var_name in hooked_vars:
                    icall_signatures.add(full_sig)

    return sorted(list(managed_patterns)), sorted(list(icall_signatures))

def read_uleb128(data, offset):
    result = 0
    shift = 0
    while True:
        b = data[offset]
        offset += 1
        result |= (b & 0x7F) << shift
        shift += 7
        if (b & 0x80) == 0:
            break
    return result, offset

def _search_trie(data, node_offset, target):
    if node_offset >= len(data):
        return None
    terminal_size, pos = read_uleb128(data, node_offset)

    if target == "":
        if terminal_size > 0:
            flags, pos = read_uleb128(data, pos)
            if (flags & 0x08) == 0 and (flags & 0x10) == 0:
                addr, _ = read_uleb128(data, pos)
                return addr
        return None

    child_pos = pos + terminal_size
    if child_pos >= len(data):
        return None
    child_count = data[child_pos]
    child_pos += 1

    for _ in range(child_count):
        edge_chars = []
        while child_pos < len(data) and data[child_pos] != 0:
            edge_chars.append(chr(data[child_pos]))
            child_pos += 1
        child_pos += 1
        edge_str = "".join(edge_chars)
        next_node_offset, child_pos = read_uleb128(data, child_pos)

        if target.startswith(edge_str):
            res = _search_trie(data, next_node_offset, target[len(edge_str):])
            if res is not None:
                return res
    return None

def find_il2cpp_init_offset(binary_path):
    try:
        with open(binary_path, "rb") as f:
            header = f.read(32)
            magic = struct.unpack("<I", header[0:4])[0]
            if magic not in (0xFEEDFACF, 0xFEEDFACE):
                return None

            ncmds = struct.unpack("<I", header[16:20])[0]
            sizeofcmds = struct.unpack("<I", header[20:24])[0]
            cmds_data = f.read(sizeofcmds)

        offset = 0
        export_off = 0
        export_size = 0

        for _ in range(ncmds):
            if offset + 8 > len(cmds_data):
                break
            cmd, cmdsize = struct.unpack("<II", cmds_data[offset:offset+8])

            if cmd in (0x22, 0x80000022):
                export_off = struct.unpack("<I", cmds_data[offset+40:offset+44])[0]
                export_size = struct.unpack("<I", cmds_data[offset+44:offset+48])[0]
            elif cmd in (0x33, 0x80000033):
                export_off = struct.unpack("<I", cmds_data[offset+8:offset+12])[0]
                export_size = struct.unpack("<I", cmds_data[offset+12:offset+16])[0]

            offset += cmdsize

        if export_off == 0 or export_size == 0:
            return None

        with open(binary_path, "rb") as f:
            f.seek(export_off)
            trie_data = f.read(export_size)

        for candidate in ["_il2cpp_init", "il2cpp_init"]:
            addr = _search_trie(trie_data, 0, candidate)
            if addr is not None:
                return addr

    except Exception as e:
        print(f"[-] Warning: Fast export parser error: {e}")
    return None

def find_caves(binary_path):
    with open(binary_path, "rb") as f:
        header = f.read(32)
        ncmds, sizeofcmds = struct.unpack("<II", header[16:24])
        cmds = f.read(sizeofcmds)

    offset = 0
    caves = {}

    for _ in range(ncmds):
        cmd, cmdsize = struct.unpack("<II", cmds[offset:offset+8])
        if cmd == 0x19:
            segname = cmds[offset+8:offset+24].split(b'\0')[0].decode('latin1')
            vmaddr, vmsize, fileoff, filesize = struct.unpack("<QQQQ", cmds[offset+24:offset+56])
            nsects = struct.unpack("<I", cmds[offset+64:offset+68])[0]

            if segname in ("__TEXT", "__DATA"):
                last_end = 0
                sec_off = offset + 72
                for _ in range(nsects):
                    s_addr, s_size = struct.unpack("<QQ", cmds[sec_off+32:sec_off+48])
                    if (s_addr + s_size) > last_end:
                        last_end = s_addr + s_size
                    sec_off += 80

                cave_va = (last_end + 15) & ~15
                seg_end = vmaddr + vmsize
                cave_size = seg_end - cave_va

                if segname == "__TEXT":
                    caves["text_offset"] = cave_va
                    caves["text_capacity"] = cave_size // 32
                elif segname == "__DATA":
                    caves["data_va"] = cave_va
                    caves["data_capacity"] = cave_size // 16

        offset += cmdsize

    capacity = min(caves["text_capacity"], caves["data_capacity"])
    print(f"[+] Auto-detected Caves: __TEXT={hex(caves['text_offset'])}, __DATA={hex(caves['data_va'])}, Capacity={capacity} hooks")
    return caves["text_offset"], caves["data_va"], capacity

def decode_adrp_add(data, pc, expected_reg=None):
    if pc + 8 > len(data):
        return None

    insn1 = struct.unpack('<I', data[pc:pc+4])[0]
    insn2 = struct.unpack('<I', data[pc+4:pc+8])[0]

    if (insn1 & 0x9F000000) != 0x90000000:
        return None
    rd1 = insn1 & 0x1F

    if expected_reg is not None and rd1 != expected_reg:
        return None

    if (insn2 & 0xFFC00000) != 0x91000000:
        return None
    rn2 = (insn2 >> 5) & 0x1F
    rd2 = insn2 & 0x1F

    if rd1 != rn2 or rd1 != rd2:
        return None

    immlo = (insn1 >> 29) & 0x3
    immhi = (insn1 >> 5) & 0x7FFFF
    imm = (immhi << 2) | immlo
    if imm & 0x100000:
        imm -= 0x200000

    pc_page = pc & ~0xFFF
    adrp_page = pc_page + (imm << 12)
    add_imm = (insn2 >> 10) & 0xFFF

    return adrp_page + add_imm

def scan_native_icalls(binary_path, icall_signatures):
    with open(binary_path, "rb") as f:
        data = f.read()

    header = data[:32]
    ncmds, sizeofcmds = struct.unpack("<II", header[16:24])
    cmds = data[32:32+sizeofcmds]

    text_start = 0
    text_end = 0

    offset = 0
    for _ in range(ncmds):
        cmd, cmdsize = struct.unpack("<II", cmds[offset:offset+8])
        if cmd == 0x19:
            segname = cmds[offset+8:offset+24].split(b'\0')[0].decode('latin1')
            if segname == "__TEXT":
                nsects = struct.unpack("<I", cmds[offset+64:offset+68])[0]
                sec_off = offset + 72
                for _ in range(nsects):
                    s_name = cmds[sec_off:sec_off+16].split(b'\0')[0].decode('latin1')
                    s_addr, s_size = struct.unpack("<QQ", cmds[sec_off+32:sec_off+48])
                    if s_name == "__text":
                        text_start = s_addr
                        text_end = s_addr + s_size
                        break
                    sec_off += 80
        offset += cmdsize

    results = {}

    for sig in icall_signatures:
        candidates = [
            sig.split('(')[0].encode('utf-8') + b'\0',
            sig.encode('utf-8') + b'\0'
        ]

        str_va = -1
        for cand in candidates:
            pos = data.find(cand)
            if pos != -1:
                str_va = pos
                break

        if str_va == -1:
            print(f"  [-] String not found in binary: {sig.split('(')[0]}")
            continue

        found_func = None
        str_page_off = str_va & 0xFFF

        add_x0_needle = struct.pack('<I', 0x91000000 | (str_page_off << 10))

        pos = text_start
        while True:
            pos = data.find(add_x0_needle, pos, text_end)
            if pos == -1:
                break

            if pos >= 4:
                resolved_str = decode_adrp_add(data, pos - 4, expected_reg=0)
                if resolved_str == str_va:
                    stub_pc = pos - 4

                    for forward_pc in range(stub_pc + 8, min(stub_pc + 24, text_end - 8), 4):
                        candidate_func = decode_adrp_add(data, forward_pc, expected_reg=1)
                        if candidate_func and text_start <= candidate_func < text_end:
                            found_func = candidate_func
                            break

                    if found_func:
                        break

            pos += 4

        if not found_func:
            ptr_needle = struct.pack('<Q', str_va)
            pos = 0
            while True:
                pos = data.find(ptr_needle, pos)
                if pos == -1:
                    break
                if pos + 16 <= len(data):
                    cand = struct.unpack('<Q', data[pos+8:pos+16])[0]
                    if text_start <= cand < text_end:
                        found_func = cand
                        break
                pos += 8

        if found_func:
            results[sig] = found_func
            print(f"  [+] Matched Native ICall: {sig.split('(')[0]} -> Offset: {hex(found_func)}")
        else:
            print(f"  [-] Could not resolve native function for: {sig.split('(')[0]}")

    return results

def main():
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <script.json> <UnityFramework_binary> [output_hooks.json]")
        sys.exit(1)

    script_json_path = sys.argv[1]
    binary_path = sys.argv[2]
    out_json_path = sys.argv[3] if len(sys.argv) > 3 else "hooks.json"

    print(f"Loading {script_json_path}...")
    with open(script_json_path, 'r', encoding='utf-8') as f:
        data = json.load(f)

    methods = data.get("ScriptMethod", [])
    print(f"Loaded {len(methods)} methods from script.json.")

    text_cave, data_cave_va, max_capacity = find_caves(binary_path)

    config = {
        "caves": {
            "text_offset": text_cave,
            "data_va": data_cave_va
        },
        "hooks": []
    }

    managed_patterns, icall_signatures = extract_patterns_and_icalls_from_rust()
    print(f"Extracted {len(managed_patterns)} managed patterns and {len(icall_signatures)} icall signatures from Rust (target_os={TARGET_OS}).")

    matched_offsets = set()

    print("\n--- Matching Managed IL2CPP Methods ---")
    for pattern in managed_patterns:
        regex = re.compile(pattern)
        found = False

        for m in methods:
            name = m.get("Name", "")
            if regex.search(name):
                addr = m.get("Address", 0)
                offset = addr - 0x100000000 if addr >= 0x100000000 else addr

                if offset not in matched_offsets:
                    config["hooks"].append({
                        "name": name,
                        "offset": offset
                    })
                    matched_offsets.add(offset)
                    print(f"  [+] Matched: {name} -> Offset: {hex(offset)}")
                    found = True

        if not found:
            print(f"  [-] Pattern not matched: {pattern}")

    print("\n--- Scanning Native C++ Engine ICalls ---")
    icall_results = scan_native_icalls(binary_path, icall_signatures)
    for sig, func_va in icall_results.items():
        if func_va not in matched_offsets:
            config["hooks"].append({
                "name": f"ICALL:{sig.split('(')[0]}",
                "offset": func_va
            })
            matched_offsets.add(func_va)

    init_offset = find_il2cpp_init_offset(binary_path)
    if init_offset:
        if init_offset not in matched_offsets:
            config["hooks"].append({
                "name": "il2cpp_init",
                "offset": init_offset
            })
            matched_offsets.add(init_offset)
            print(f"  [+] Added native symbol: il2cpp_init -> Offset: {hex(init_offset)}")
    else:
        print("  [-] Could not auto-detect il2cpp_init! (Add it manually if needed)")

    config["hooks"].sort(key=lambda h: h["offset"])

    for i in range(len(config["hooks"]) - 1):
        curr_hook = config["hooks"][i]
        next_hook = config["hooks"][i + 1]
        diff = next_hook["offset"] - curr_hook["offset"]
        if diff < 4:
            print(f"\n[-] ERROR: Overlapping hooks detected!")
            print(f"    '{curr_hook['name']}' ({hex(curr_hook['offset'])}) and '{next_hook['name']}' ({hex(next_hook['offset'])})")
            print(f"    Distance is {diff} bytes, but each AOT stub requires 4 bytes! Aborting.")
            sys.exit(1)

    if len(config["hooks"]) > max_capacity:
        print(f"\n[-] ERROR: Hook count ({len(config['hooks'])}) exceeds cave capacity ({max_capacity})!")
        sys.exit(1)

    with open(out_json_path, 'w', encoding='utf-8') as f:
        json.dump(config, f, indent=2)

    print(f"\nSuccessfully generated {out_json_path} with {len(config['hooks'])} hooks.")

if __name__ == "__main__":
    main()