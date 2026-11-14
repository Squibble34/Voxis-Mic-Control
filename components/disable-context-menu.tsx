"use client"

import * as React from "react"

export function DisableContextMenu() {
  React.useEffect(() => {
    const suppressContextMenu = (event: MouseEvent) => {
      event.preventDefault()
    }
    window.addEventListener("contextmenu", suppressContextMenu, true)
    return () => {
      window.removeEventListener("contextmenu", suppressContextMenu, true)
    }
  }, [])

  return null
}
