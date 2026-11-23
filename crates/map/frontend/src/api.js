// The map server's JSON endpoints. Every failure carries the server's own message.

async function call(path, options) {
  const response = await fetch(path, options)
  const body = await response.json().catch(() => ({}))
  if (!response.ok) throw new Error(body.error ?? `${path} answered ${response.status}`)
  return body
}

const query = (params) => new URLSearchParams(params).toString()

export const fetchLevel = (parent) => call(`/api/level${parent ? `?${query({ parent })}` : ""}`)
export const fetchNode = (id) => call(`/api/node?${query({ id })}`)
export const search = (q, signal) => call(`/api/search?${query({ q })}`, { signal })
export const describe = (id) => call(`/api/describe?${query({ id })}`, { method: "POST" })
