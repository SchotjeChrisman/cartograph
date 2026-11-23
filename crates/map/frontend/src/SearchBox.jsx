import { useAsyncOptions } from "@ui-sdk/react/async-options"
import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxStatus,
} from "@ui-sdk/react/combobox"
import { search } from "./api.js"

const DEBOUNCE_MS = 250

async function findHits(text, { signal }) {
  if (text.trim() === "") return []
  const hits = await search(text, signal)
  return hits.map((hit) => ({ value: `${hit.kind}:${hit.target}:${hit.label}`, label: hit.label, hit }))
}

/** Type a component or symbol name; choosing a result opens its component. */
export function SearchBox({ onChoose }) {
  const results = useAsyncOptions(findHits, { debounce: DEBOUNCE_MS })
  return (
    <Combobox
      items={results.items}
      filter={false}
      status={results.status}
      onQueryChange={(text) => results.setQuery(text)}
      onValueChange={(value) => {
        const chosen = results.items.find((item) => item.value === value)
        if (chosen) onChoose(chosen.hit)
      }}
    >
      <ComboboxInput placeholder="Search components and symbols" aria-label="Search" />
      <ComboboxContent>
        <ComboboxStatus>
          {(state) => (state.status === "error" ? "Search failed." : state.loading ? "Searching…" : "")}
        </ComboboxStatus>
        <ComboboxEmpty>{(state) => (state.empty && !state.loading ? "No matches." : null)}</ComboboxEmpty>
        <ComboboxList>
          {(state) =>
            state.items.map((item) => (
              <ComboboxItem key={item.value} value={item.value} label={item.label}>
                <span className="hit-label">{item.label}</span>
                <span className="hit-detail">{item.hit.detail}</span>
              </ComboboxItem>
            ))
          }
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  )
}
