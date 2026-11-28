---
name: grill-me
description: Interview the user to resolve a stuck feature, system design, or folder-structure decision when they ask to be grilled or request a design interview.
---

# Grill Me

Clarify the design before implementation. Ask for the desired outcome and the decision the user is stuck on if they have
not already provided them. In the Game Engine repository, use the applicable `AGENTS.md`, project status, source, and
relevant architecture decisions to establish what already exists.

1. Identify the decisions that affect scope, behavior, ownership, or acceptance. Resolve prerequisites first. Separate
   decisions the user owns from facts you can check in code, tests, or documentation. Use an independent read-only agent
   for fact-finding when it helps.
2. Ask the most consequential question that can be answered now. Group independent questions only if the user can answer
   them together. Give a recommendation with its reason and the main tradeoff. Wait for the user's answers before asking
   dependent questions. Update the remaining decisions when an answer changes the design.
3. When the material decisions are settled, summarize the chosen design, acceptance checks, remaining uncertainties, and
   the first implementation step. Confirm the summary with the user before implementation. When handing off across
   chats, record the confirmed decisions and next step using the repo's coordination rules.

Adapted from Matt Pocock's `grill-me` and `grilling` skills. See [LICENSE](LICENSE).
