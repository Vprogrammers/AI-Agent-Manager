"""Conservative prompt preparation. Never rewrite user requirements or infer intent."""
import re
import subprocess

def relevant_files(root, prompt, limit=8):
    """Suggest tracked paths only; never inject file contents or secrets."""
    result = subprocess.run(['git', '-C', str(root), 'ls-files', '-z'], capture_output=True, check=False)
    if result.returncode:
        return []
    terms = set(re.findall(r'[a-zA-Z][a-zA-Z0-9_]{3,}', prompt.lower()))
    if not terms:
        return []
    candidates = []
    for raw in result.stdout.split(b'\0'):
        if not raw:
            continue
        path = raw.decode('utf-8', errors='replace')
        if any(part in path.lower() for part in ('node_modules/', '.gradle/', 'build/', 'dist/', '.env', 'secret', 'credential')):
            continue
        score = sum(term in path.lower() for term in terms)
        if score:
            candidates.append((score, path))
    return [path for _, path in sorted(candidates, key=lambda item: (-item[0], item[1]))[:limit]]

def prepare(prompt, root=None, existing_context=False):
    """Only normalize redundant whitespace and repeated identical lines.
    Never remove requirements, code blocks, or quoted text."""
    original = prompt
    # Exact duplicate nonempty lines can be removed only outside fenced code.
    lines = prompt.splitlines()
    seen = set()
    result = []
    fenced = False
    removed = 0
    for line in lines:
        if line.lstrip().startswith(('\\x60\\x60\\x60', '~~~')):
            fenced = not fenced
            result.append(line)
            continue
        key = line.strip()
        if not fenced and key and key in seen and len(key) >= 24:
            removed += 1
            continue
        result.append(line)
        if not fenced and key:
            seen.add(key)
    optimized = '\n'.join(result)
    # Preserve original when no safe reduction is possible.
    files = relevant_files(root, original) if root else []
    return {'original': original, 'optimized': optimized, 'changed': optimized != original,
            'removed_duplicate_lines': removed, 'relevant_files': files,
            'session_context_reused': bool(existing_context),
            'estimated_character_reduction': len(original) - len(optimized),
            'token_savings': None,
            'quality_verified': False}
