# Agent guidelines for plotui

## Protected human-edited content

Some text in this repo has been hand-written or hand-tuned by a human and is
marked with `HUMAN-EDITED` markers. **Never rewrite, reword, delete, or
"improve" anything between these markers without explicitly asking the user
first and receiving permission.** Mechanical changes that don't alter the text
(re-indenting, moving the block intact, fixing surrounding markup) are fine.

Marker syntax by file type:

HTML (e.g. `site/index.html`):

```html
<!-- HUMAN-EDITED: do not modify without permission -->
<p>...protected text...</p>
<!-- /HUMAN-EDITED -->
```

MDX (e.g. `docs/content/docs/*.mdx` — HTML comments are invalid in MDX):

```mdx
{/* HUMAN-EDITED: do not modify without permission */}
...protected text...
{/* /HUMAN-EDITED */}
```

Markdown, code comments, and everything else:

```
<!-- HUMAN-EDITED --> ... <!-- /HUMAN-EDITED -->   (Markdown)
// HUMAN-EDITED ... // /HUMAN-EDITED               (code)
```

If a requested change would require touching a protected block, stop and ask
the user before editing it. When asked to review or rewrite a whole file,
leave protected blocks verbatim and say you skipped them.
