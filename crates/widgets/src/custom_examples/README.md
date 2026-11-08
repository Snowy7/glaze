# Your own native widgets

Studio's **Widgets → New custom widget** installs these examples in
`%APPDATA%\glaze\widgets`, creates another definition and opens it in your editor. Existing files
are preserved. Each `name.json` appears as a separate `custom-name` entry in both widget pickers.
Open the picker again after creating or renaming a file. You can add several instances of each.

```json
{
  "title": "My number",
  "layout": "stat",
  "source": {
    "type": "url",
    "url": "https://example.com/status.json",
    "path": "$.stats.count"
  },
  "refreshSeconds": 300,
  "size": [4, 3],
  "suffix": " items",
  "caption": "A small useful thing"
}
```

Layouts: `stat`, `list`, `text`, `image`, `gauge`. A list accepts a JSON array or newline-separated
text; array objects can have `title` or `name`. A gauge accepts a number from 0 to 100 or
`{"value": 42, "max": 80}`. An image accepts a local image path, an HTTPS image URL, or `{"url": ...}`.

Sources:

```json
{"type":"static","value":"Keep it simple"}
{"type":"url","url":"https://example.com/data.json","path":"$.items[*].name"}
{"type":"file","path":"my-data.json","jsonPath":"$.items[0]"}
{"type":"command","program":"my-tool.exe","args":["--json"],"jsonPath":"$.value"}
```

`jsonPath` is optional for files and commands; without it, JSON is parsed or the output is plain
text. Supported JSONPath: `$`, `.key`, `[index]`, `["quoted.key"]`, `['quoted.key']`, `[*]` and `.*`.
Filters and recursive descent are rejected. Relative file/image paths and a command's working
directory resolve to the widgets folder. For an image file itself, use an image layout with a
file source; for a text/JSON file containing an image path, select the path with `jsonPath`.

Refresh is 10–86400 seconds, or `0` for manual refresh. Work happens only while the instance is
visible. The refresh button also reloads the definition; invalid updates retain the last displayed
data and show the error. Commands run as your Windows user, with no implicit shell, a five-second
timeout, a kill-on-close process job and bounded stdout; this is not a sandbox. Definitions/data are
limited to 256 KiB; images to 8 MiB and 1024×1024 decoded pixels. HTTPS requests do not follow
redirects. Dimensions can be any whole cells from 2×2 to 12×12.

Examples: a focus note, the latest Rust release from GitHub, a local reading list and disk usage
from an explicitly configured PowerShell command. Edit/copy them freely; choose a different
filename for a separate catalog entry.
