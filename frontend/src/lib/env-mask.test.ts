import { describe, expect, it } from "vitest"

import { maskEnv } from "@/lib/env-mask"

describe("maskEnv", () => {
  it("hides the value but keeps the key", () => {
    expect(maskEnv("TOKEN=s3cr3t")).toBe("TOKEN=••••••")
  })

  it("leaves comments and blank lines alone", () => {
    const input = "# database\n\nDB_URL=postgres://u:p@h/db"
    expect(maskEnv(input)).toBe("# database\n\nDB_URL=••••••••••••••••")
  })

  it("keeps an empty value empty", () => {
    expect(maskEnv("EMPTY=")).toBe("EMPTY=")
    expect(maskEnv("SPACES=   ")).toBe("SPACES=   ")
  })

  it("does not touch a line without an equals sign", () => {
    expect(maskEnv("not a pair")).toBe("not a pair")
  })

  it("masks only the first equals sign onwards", () => {
    expect(maskEnv("KEY=a=b=c")).toBe("KEY=•••••")
  })

  it("does not leak the value length beyond sixteen characters", () => {
    const masked = maskEnv(`LONG=${"x".repeat(200)}`)
    expect(masked).toBe(`LONG=${"•".repeat(16)}`)
  })

  it("pads short values so a one-character secret is not obvious", () => {
    expect(maskEnv("SHORT=a")).toBe("SHORT=••••")
  })
})
