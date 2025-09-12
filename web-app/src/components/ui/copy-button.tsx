import { useEffect } from "react";
import { useState } from "react";
import { Button } from "./button";
import { HiOutlineClipboard, HiMiniCheck } from "react-icons/hi2";

function CopyButton({ onCopy }: { onCopy: () => void }) {
  const [isCopied, setIsCopied] = useState(false);

  useEffect(() => {
    if (isCopied) {
      setTimeout(() => {
        setIsCopied(false);
      }, 1500);
    }
  }, [isCopied]);

  return (
    <Button
      type="button"
      className="size- flex-shrink-0"
      size="icon"
      variant="outline"
      onClick={() => {
        onCopy();
        setIsCopied(true);
      }}
    >
      {isCopied ? <HiMiniCheck className="size-5" /> : <HiOutlineClipboard className="size-5" />}
    </Button>
  );
}

export default CopyButton;
