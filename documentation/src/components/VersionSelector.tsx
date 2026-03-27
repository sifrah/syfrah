'use client'

// Full multi-version support (deploy per tag) planned for future release

import { useEffect, useState } from 'react'

interface VersionInfo {
  version: string
}

export function VersionSelector() {
  const [version, setVersion] = useState<string | null>(null)

  useEffect(() => {
    fetch('/version.json')
      .then((res) => res.json())
      .then((data: VersionInfo) => setVersion(data.version))
      .catch(() => setVersion('dev'))
  }, [])

  if (!version) {
    return null
  }

  return (
    <div className="flex items-center">
      <span className="inline-flex items-center gap-1 rounded-full bg-zinc-100 px-2.5 py-0.5 text-xs font-medium text-zinc-700 ring-1 ring-inset ring-zinc-300 dark:bg-zinc-800 dark:text-zinc-300 dark:ring-zinc-700">
        {version}
      </span>
    </div>
  )
}
