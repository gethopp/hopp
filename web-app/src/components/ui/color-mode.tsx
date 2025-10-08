import { createContext, useContext, useEffect, useState } from 'react'

type ColorMode = 'light' | 'dark'
type ColorModeContextType = {
  colorMode: ColorMode
  toggleColorMode: () => void
}

const ColorModeContext = createContext<ColorModeContextType | undefined>(undefined)

export function ColorModeProvider({ children }: { children: React.ReactNode }) {
  const [colorMode, setColorMode] = useState<ColorMode>(() => {
    // Check localStorage and system preference
    const savedMode = localStorage.getItem('color-mode') as ColorMode
    if (savedMode) return savedMode
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  })

  useEffect(() => {
    const root = window.document.documentElement
    root.classList.remove('light', 'dark')
    root.classList.add(colorMode)
    localStorage.setItem('color-mode', colorMode)
  }, [colorMode])

  const toggleColorMode = () => {
    setColorMode(prev => prev === 'light' ? 'dark' : 'light')
  }

  return (
    <ColorModeContext.Provider value={{ colorMode, toggleColorMode }}>
      {children}
    </ColorModeContext.Provider>
  )
}

export function useColorMode() {
  const context = useContext(ColorModeContext)
  if (context === undefined) {
    throw new Error('useColorMode must be used within a ColorModeProvider')
  }
  return context
}