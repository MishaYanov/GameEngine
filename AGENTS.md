# Documentation maintenance

For documentation work, inspect Git history and the current Git tree, including staged, unstaged, and untracked changes. Use `docs-site/docs-state.json` to record the last inspected commit and pending documentation changes. That checkpoint is an inspection baseline, not proof that documentation matches the commit.

Update documentation only when the user explicitly commands an update. Compare against the recorded baseline and also inspect uncommitted changes, since HEAD can remain unchanged while source evolves. After a requested update, rebuild the static site, run its checks, and record the documentation checkpoint and any verification limits.

Existing engine documentation belongs to the Engine Developer space. Keep Game App Developer empty unless the user requests content there. Keep Markdown as the source and generated site output in `docs-site/dist`.

# Project agent rules and skills

Read [.agents/rules/project.md](.agents/rules/project.md) before changing this project. The rules and relevant reusable
skills are indexed in [.agents/README.md](.agents/README.md). For editor code, also
follow [apps/editor/AGENTS.md](apps/editor/AGENTS.md). Load only the skill needed for the current task.
