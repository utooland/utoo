// Adapted from next.js/apps/bundle-analyzer/components/route-typeahead.tsx
// at f77675fd18 (MIT; see LICENSE.next.js.md in the built analyzer).

import { Check, ChevronsUpDown, Route } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Kbd } from "@/components/ui/kbd";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export interface Entry {
  name: string;
  path: string;
  environment: "client" | "server";
}

export function EntryPicker({
  entries,
  selectedEntry,
  onSelect,
}: {
  entries: Entry[];
  selectedEntry: Entry;
  onSelect: (entry: Entry) => void;
}) {
  const [open, setOpen] = useState(false);
  const [shortcutLabel, setShortcutLabel] = useState<string | null>(null);

  useEffect(() => {
    const isAppleDevice = /Mac|iPhone|iPad|iPod/.test(navigator.userAgent);
    setShortcutLabel(isAppleDevice ? "⌘K" : "Ctrl+K");

    const handleKeyDown = (event: KeyboardEvent) => {
      const activeElement = document.activeElement;
      if (activeElement?.matches("input, textarea, [contenteditable=true]")) {
        return;
      }
      const modifier = isAppleDevice ? event.metaKey : event.ctrlKey;
      if (modifier && event.key === "k") {
        event.preventDefault();
        setOpen(true);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  return (
    <div className="flex min-w-0 items-center gap-2 sm:min-w-64">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            variant="outline"
            role="combobox"
            aria-expanded={open}
            aria-label={`Select entry. Current entry: ${selectedEntry.name}`}
            className="w-full min-w-0 justify-between font-mono text-sm"
          >
            <div className="flex min-w-0 flex-1 items-center">
              <Route className="inline mr-2 shrink-0" />
              <span
                className="min-w-0 flex-1 truncate"
                title={selectedEntry.name}
              >
                {truncateMiddle(selectedEntry.name, 32)}
              </span>
            </div>
            <div className="ml-2 flex shrink-0 items-center gap-2">
              {shortcutLabel && <Kbd>{shortcutLabel}</Kbd>}
              <ChevronsUpDown className="h-4 w-4 shrink-0 opacity-50" />
            </div>
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="w-[40rem] max-w-[calc(100vw-1rem)] overflow-hidden p-0"
        >
          <Command className="min-w-0">
            <CommandInput placeholder="Search entries..." className="h-9" />
            <CommandList className="min-w-0">
              <CommandEmpty>No entry found.</CommandEmpty>
              <CommandGroup className="min-w-0 [&_[cmdk-group-items]]:min-w-0">
                {entries.map((entry) => (
                  <CommandItem
                    key={entry.path}
                    value={entry.path}
                    keywords={[entry.name]}
                    className="w-full min-w-0 overflow-hidden font-mono"
                    onSelect={() => {
                      onSelect(entry);
                      setOpen(false);
                    }}
                  >
                    <Check
                      className={cn(
                        "mr-2 h-4 w-4 shrink-0",
                        selectedEntry.path === entry.path
                          ? "opacity-100"
                          : "opacity-0",
                      )}
                    />
                    <span className="sr-only">{entry.name}</span>
                    <span
                      aria-hidden="true"
                      className="min-w-0 flex-1 truncate"
                      title={entry.name}
                    >
                      {truncateMiddle(entry.name, 64)}
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
    </div>
  );
}

function truncateMiddle(value: string, maxLength: number): string {
  if (value.length <= maxLength) return value;
  const startLength = Math.ceil((maxLength - 1) / 2);
  const endLength = Math.floor((maxLength - 1) / 2);
  return `${value.slice(0, startLength)}…${value.slice(-endLength)}`;
}
