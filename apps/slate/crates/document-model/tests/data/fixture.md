# Slate Markdown fixture

A paragraph with *italic*, **bold**, ***bold italic***, ~~struck~~, `inline code`,
and a [link](https://example.com/slate "Example") across a soft break.
A hard break follows this line  
and this line continues the paragraph.

## Second level heading

### Third level heading

#### Fourth level heading

##### Fifth level heading

###### Sixth level heading

> A block quote with **bold** inside.
>
> > A nested quote.

- First bullet
- Second bullet
  - Nested bullet
    - Third level bullet
- Back to the first level

1. First number
2. Second number
   1. Nested number
3. Third number

A list that starts at seven:

7. Seven starts this list
8. Eight follows

- [ ] An open task
- [x] A done task

```rust
fn main() {
    println!("hello");
}
```

    indented code block

| Name | Count | Note |
| :--- | :---: | ---: |
| Apples | 3 | *fresh* |
| Pears | 12 | `ripe` |

---

![A blue picture](picture.png)

<div class="note">
Raw <b>HTML</b> block.
</div>

Text with inline <kbd>HTML</kbd> and a reference [link][ref] and a footnote[^1].

[ref]: https://example.com/ref "Reference"

[^1]: The footnote's text.

The last paragraph.
