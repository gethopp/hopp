import { Button } from "@/components/ui/button"
import { useColorMode } from "./color-mode"
import { LuMoon, LuSun } from "react-icons/lu"

export const ColorModeButton = () => {
  const { toggleColorMode, colorMode } = useColorMode()
  return (
    <Button 
      onClick={toggleColorMode} 
      variant="outline" 
      size="icon"
      className="size-9"
      aria-label="Toggle color mode"
    >
      {colorMode === "light" ? <LuSun className="h-[1.2rem] w-[1.2rem]" /> : <LuMoon className="h-[1.2rem] w-[1.2rem]" />}
    </Button>
  )
}