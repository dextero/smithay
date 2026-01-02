## Gemini Added Memories

- Use `jj commit -m "commit description"` to create jj commits instead of `jj describe`.
- After completing each task or significant sub-task, create a jj commit with a descriptive message.
- After each change verified to build and pass tests without benchmark regressions, create a jj commit with description and a summary of benchmark results.
- wgpu in this project tends to randomly fail with "SIGSEGV: invalid memory reference". Rerun tests to check for flakes before debugging this specific error.
