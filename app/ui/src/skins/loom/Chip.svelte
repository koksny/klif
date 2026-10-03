<script lang="ts">
  // Scene text in Loom's two voices: words in Archivo Expanded caps, numbers in JetBrains Mono
  // ("kv cache · 66 098 tok · 67%" -> KV CACHE · 66 098 TOK · 67%). Size and colour come from the parent.
  let { text }: { text: string } = $props();

  // a number: digits with thin-space thousands, a decimal point or a clock colon, an optional percent sign
  const NUM = /(\d[\d .,:]*\d%?|\d%?)/;
  const parts = $derived(
    text
      .split(NUM)
      .filter((t) => t.length > 0)
      .map((t) => ({ t, n: /\d/.test(t) })),
  );
</script>

{#each parts as p, i (i)}<span class={p.n ? 'n' : 'w'}>{p.t}</span>{/each}

<style>
  .w {
    font-family: 'Archivo Variable', 'Archivo', sans-serif;
    font-stretch: 125%;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.14em;
  }
  .n {
    font-family: 'JetBrains Mono Variable', Consolas, monospace;
    font-weight: 400;
    letter-spacing: 0;
    font-variant-numeric: tabular-nums;
  }
</style>
