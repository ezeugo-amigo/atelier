---
name: simplify
description: Simplify code you've just written or changed. Look for redundancies to eliminate and rewrites that make the code easier to read and maintain. Use after finishing an implementation, or whenever the user asks to simplify, clean up, or de-clutter code. Optimizes for readability and maintainability, not runtime efficiency.
---

# Simplify

Review the target code (by default, the code you just wrote or the current diff — ask if it's unclear) and rewrite it to be simpler, without changing its behavior. This is a quality pass, not a bug hunt: don't go looking for correctness issues, and don't change what the code does.

## What to look for

- **Redundancy.** Duplicated logic, repeated conditions, variables that just alias another value, code that recomputes something already available. Collapse it.
- **Unneeded abstraction.** Helper functions called once, interfaces with a single implementation, layers of indirection that don't earn their keep. Inline them.
- **Overcomplicated control flow.** Nested conditionals that can flatten with early returns, loops that a built-in (map/filter/find/some/every, etc.) already expresses, branches that end up doing the same thing.
- **Dead weight.** Unused parameters, variables, imports, and branches that can never run.
- **Naming and shape that obscure intent.** Code that would read more clearly restructured — even if it isn't strictly "redundant" — still counts as a simplification target.

## Principles

- Fewer lines of code is better. No lines of code — deleting the construct entirely while still accomplishing the original goal — is best.
- Optimize for a reader seeing this code for the first time: can they hold the whole thing in their head? Prefer the version that's easier to follow over the version that's easier to extend for a hypothetical future need.
- This is explicitly not about performance. Don't trade away clarity for speed, and don't add complexity in the name of efficiency. If a "simpler" option is also slower and the code isn't on a hot path, take the simpler option.
- Preserve behavior exactly. Don't fix bugs you notice in passing — mention them separately instead of folding them into the simplification.
- Don't introduce new abstractions, config options, or flexibility the current code doesn't need. Simplifying is about removing, not redesigning.
- Skip comments that just restate what simpler code now makes obvious; delete them along with the complexity they were explaining.

## Process

1. Identify the scope: the diff, the file, or the function the user pointed at.
2. Read it fully before editing — a change in one spot often removes the need for code elsewhere.
3. Make the edits directly. Prefer deleting code over rewriting it, and rewriting over adding.
4. After editing, re-read the result and confirm it still does exactly what the original did — same inputs, same outputs, same side effects.
5. Summarize what was removed or simplified and why, briefly. If nothing needed simplifying, say so instead of inventing changes.
