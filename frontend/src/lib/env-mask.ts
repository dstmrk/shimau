/**
 * Masks the values in a `.env` file for display.
 *
 * `.env` is a text file to shimau and is never reinterpreted (spec §4.6), so
 * this is presentation only: masked content is never sent back to the server,
 * and the editor stays read-only until the values are revealed.
 */
export function maskEnv(content: string): string {
  return content
    .split("\n")
    .map((line) => {
      const trimmed = line.trimStart()
      if (trimmed === "" || trimmed.startsWith("#")) {
        return line
      }
      const separator = line.indexOf("=")
      if (separator === -1) {
        return line
      }
      const key = line.slice(0, separator + 1)
      const value = line.slice(separator + 1)
      if (value.trim() === "") {
        return line
      }
      return key + "•".repeat(Math.min(Math.max(value.length, 4), 16))
    })
    .join("\n")
}
