import * as React from "react";
import { Combobox, ComboboxButton, ComboboxInput, ComboboxOption, ComboboxOptions } from "@headlessui/react";
import { CheckIcon, ChevronsUpDownIcon, XIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

export interface MultiSelectOption {
  value: string;
  label: string;
}

interface MultiSelectProps {
  options: MultiSelectOption[];
  value: MultiSelectOption[];
  onChange: (value: MultiSelectOption[]) => void;
  placeholder?: string;
  searchPlaceholder?: string;
  emptyMessage?: string;
  className?: string;
  disabled?: boolean;
}

export function MultiSelect({
  options,
  value = [],
  onChange,
  placeholder = "Select items...",
  searchPlaceholder = "Search...",
  emptyMessage = "No items found.",
  className,
  disabled = false,
}: MultiSelectProps) {
  const [query, setQuery] = React.useState("");
  const [expanded, setExpanded] = React.useState(false);
  const containerRef = React.useRef<HTMLDivElement>(null);

  const filteredOptions =
    query === "" ? options : options.filter((option) => option.label.toLowerCase().includes(query.toLowerCase()));

  const removeSelection = (optionValue: string) => {
    onChange(value.filter((v) => v.value !== optionValue));
  };

  const maxShownItems = 2;
  const visibleItems = expanded ? value : value.slice(0, maxShownItems);
  const hiddenCount = value.length - visibleItems.length;

  React.useEffect(() => {
    const updateWidth = () => {
      if (containerRef.current) {
        const width = containerRef.current.offsetWidth;
        containerRef.current.style.setProperty("--input-width", `${width}px`);
      }
    };

    updateWidth();

    // Update width when window resizes or content changes
    window.addEventListener("resize", updateWidth);
    const resizeObserver = new ResizeObserver(updateWidth);
    if (containerRef.current) {
      resizeObserver.observe(containerRef.current);
    }

    return () => {
      window.removeEventListener("resize", updateWidth);
      resizeObserver.disconnect();
    };
  }, [value, expanded]);

  return (
    <Combobox value={value} onChange={onChange} multiple disabled={disabled} onClose={() => setQuery("")}>
      <div ref={containerRef} className={cn("relative", className)}>
        <ComboboxButton
          as="div"
          className={cn(
            "flex h-auto min-h-9 w-full items-center justify-between rounded-md border border-input bg-background px-3 py-2 text-sm shadow-xs outline-none transition-all",
            "hover:bg-accent/50 dark:bg-input/30 dark:border-input dark:hover:bg-input/50",
            "focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]",
            disabled && "cursor-not-allowed opacity-50",
          )}
        >
          <div className="flex flex-1 flex-wrap items-center gap-1">
            {value.length > 0 ?
              <>
                {visibleItems.map((option) => (
                  <Badge key={option.value} variant="outline" className="gap-1">
                    {option.label}
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-4 hover:bg-transparent"
                      onClick={(e: React.MouseEvent) => {
                        e.stopPropagation();
                        removeSelection(option.value);
                      }}
                      asChild
                    >
                      <span>
                        <XIcon className="size-3" />
                      </span>
                    </Button>
                  </Badge>
                ))}
                {hiddenCount > 0 || expanded ?
                  <Badge
                    variant="outline"
                    className="cursor-pointer"
                    onClick={(e: React.MouseEvent) => {
                      e.stopPropagation();
                      setExpanded((prev) => !prev);
                    }}
                  >
                    {expanded ? "Show Less" : `+${hiddenCount} more`}
                  </Badge>
                : null}
              </>
            : <span className="text-muted-foreground">{placeholder}</span>}
          </div>
          <ChevronsUpDownIcon className="ml-2 size-4 shrink-0 text-muted-foreground/80" aria-hidden="true" />
        </ComboboxButton>

        <ComboboxOptions
          anchor="bottom start"
          className={cn(
            "ease-in data-leave:data-closed:opacity-0 z-[60] w-[var(--input-width,100%)] min-w-[200px] rounded-md border border-input bg-popover p-1 shadow-lg outline-none",
            "[--anchor-gap:4px] [--anchor-padding:4px]",
          )}
        >
          <div className="relative">
            <ComboboxInput
              className={cn(
                "w-full rounded-md border-0 bg-transparent px-3 py-2 text-sm outline-none",
                "placeholder:text-muted-foreground",
              )}
              placeholder={searchPlaceholder}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              displayValue={() => ""}
            />
          </div>

          {filteredOptions.length === 0 && query !== "" ?
            <div className="px-3 py-2 text-sm text-muted-foreground">{emptyMessage}</div>
          : filteredOptions.map((option) => (
              <ComboboxOption
                key={option.value}
                value={option}
                className="group relative flex cursor-pointer select-none items-center justify-between rounded-sm px-3 py-2 text-sm outline-none data-focus:bg-accent data-focus:text-accent-foreground transition-colors"
              >
                <span className="truncate">{option.label}</span>
                <CheckIcon className="invisible size-4 shrink-0 group-data-selected:visible" />
              </ComboboxOption>
            ))
          }
        </ComboboxOptions>
      </div>
    </Combobox>
  );
}

// Single select variant
interface SelectProps {
  options: MultiSelectOption[];
  value: MultiSelectOption | null;
  onChange: (value: MultiSelectOption | null) => void;
  placeholder?: string;
  searchPlaceholder?: string;
  emptyMessage?: string;
  className?: string;
  disabled?: boolean;
}

export function Select({
  options,
  value,
  onChange,
  placeholder = "Select an item...",
  searchPlaceholder = "Search...",
  emptyMessage = "No items found.",
  className,
  disabled = false,
}: SelectProps) {
  const [query, setQuery] = React.useState("");

  const filteredOptions =
    query === "" ? options : options.filter((option) => option.label.toLowerCase().includes(query.toLowerCase()));

  return (
    <Combobox value={value} onChange={onChange} disabled={disabled} onClose={() => setQuery("")}>
      <div className={cn("relative", className)}>
        <ComboboxButton
          as="div"
          className={cn(
            "flex h-9 w-full items-center justify-between rounded-md border border-input bg-background px-3 py-2 text-sm shadow-xs outline-none transition-all",
            "hover:bg-accent/50 dark:bg-input/30 dark:border-input dark:hover:bg-input/50",
            "focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]",
            disabled && "cursor-not-allowed opacity-50",
          )}
        >
          <span className={cn(!value && "text-muted-foreground")}>{value?.label || placeholder}</span>
          <ChevronsUpDownIcon className="ml-2 size-4 shrink-0 text-muted-foreground/80" aria-hidden="true" />
        </ComboboxButton>

        <ComboboxOptions
          anchor="bottom start"
          className={cn(
            "z-[60] w-[var(--input-width)] rounded-md border border-input bg-popover p-1 shadow-lg outline-none",
            "empty:invisible",
            "[--anchor-gap:8px] [--anchor-padding:10px]",
          )}
        >
          <div className="relative">
            <ComboboxInput
              className={cn(
                "w-full rounded-md border-0 bg-transparent px-3 py-2 text-sm outline-none",
                "placeholder:text-muted-foreground",
              )}
              placeholder={searchPlaceholder}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              displayValue={(option: MultiSelectOption | null) => option?.label || ""}
            />
          </div>

          {filteredOptions.length === 0 && query !== "" ?
            <div className="px-3 py-2 text-sm text-muted-foreground">{emptyMessage}</div>
          : filteredOptions.map((option) => (
              <ComboboxOption
                key={option.value}
                value={option}
                className="group relative flex cursor-pointer select-none items-center justify-between rounded-sm px-3 py-2 text-sm outline-none data-focus:bg-accent data-focus:text-accent-foreground transition-colors"
              >
                <span className="truncate">{option.label}</span>
                <CheckIcon className="invisible size-4 shrink-0 group-data-selected:visible" />
              </ComboboxOption>
            ))
          }
        </ComboboxOptions>
      </div>
    </Combobox>
  );
}
