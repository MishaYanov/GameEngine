# Documentation maintenance

For documentation work, inspect Git history and the current Git tree, including staged, unstaged, and untracked changes. Use `docs-site/docs-state.json` to record the last inspected commit and pending documentation changes. That checkpoint is an inspection baseline, not proof that documentation matches the commit.

Update documentation only when the user explicitly commands an update. Compare against the recorded baseline and also inspect uncommitted changes, since HEAD can remain unchanged while source evolves. After a requested update, rebuild the static site, run its checks, and record the documentation checkpoint and any verification limits.

Existing engine documentation belongs to the Engine Developer space. Keep Game App Developer empty unless the user requests content there. Keep Markdown as the source and generated site output in `docs-site/dist`.
