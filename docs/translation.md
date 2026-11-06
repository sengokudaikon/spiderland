# Translation

The English is the msgid. Views and Rust share one msgid space, so the same English is translated once.

## In views

```slint
@tr("Item: {}", name)
@tr("{n} file" | "{n} files" % count)
@tr("key" => "Delete")
```

The last one has a context, which tells apart two uses of the same English.

## In Rust

Declare words in a `phrases!` block beside the code that uses them:

```rust
spiderland_phrases::phrases! {
    AddItem: "Add item"
    Added: "Added {name}."
    SealNormal: "seal" => "Normal"
    Drafts: "{n} draft" | "{n} drafts"
}

assert_eq!(AddItem.get(), "Add item");
assert_eq!(Added.with(&[("name", &"Loom")]), "Added Loom.");
assert_eq!(Drafts.count(2), "2 drafts");
```

- Values go in by name through `Phrase::with`, so a translation can move them.
- A count goes through a `Plural`, whose `{n}` picks the form.
- Never build a sentence from pieces.
- The English has to be a literal: the template is read from the sources, not from a build.

## Turning off the default context

Both sides have to leave it off, or the same English becomes two entries:

- A Slint build script calls `with_default_translation_context(DefaultTranslationContext::None)`.
- `slint-tr-extractor` gets `--no-default-translation-context`, which `Template` passes for you.

## The template

Run `Template` from a task. `write` puts every word in one `.pot`, and `check` fails if that file is out of date.

- References name files only, so moving code doesn't make the template stale.
- The header has no date, so a check compares the file as written.
- Keep `slint-tr-extractor` at the version of Slint your views build with.

## Adding a translation

1. Put it in `<lang>/<language>/LC_MESSAGES/<domain>.po`.
2. In the build script, call `bundle_catalogs`. It writes the table for the Rust phrases and returns the languages it found. With
   no folder there it bundles none and warns; an empty folder bundles none quietly. It panics on a folder it can't read.
3. Pass the same folder to `with_bundled_translations`, but only if there are any languages: Slint refuses a folder with none.
4. Call `select_language` as soon as the first window exists, before any page or message is built. Views and phrases then show in
   the same language.
5. Test that `Translations::new` takes your catalogs. `select_language` fails, and changes nothing, when a catalog's plural rule
   can't be read.
