import { useState, useId } from "react";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ChevronsUpDownIcon, XIcon, CheckIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Command, CommandEmpty, CommandGroup, CommandItem, CommandList } from "@/components/ui/command";
import { Pressable, DialogTrigger, Popover, Dialog as AriaDialog } from "react-aria-components";

export const pairingTools = [
  { value: "slack-huddle", label: "Slack (Huddle)" },
  { value: "microsoft-teams", label: "Microsoft Teams" },
  { value: "google-meet", label: "Google Meet" },
  { value: "zoom", label: "Zoom" },
  { value: "tuple", label: "Tuple, Co-screen etc." },
];

export const referralSources = [
  { value: "recommendations", label: "Recommendations (friends, colleague)" },
  { value: "internet-search", label: "Internet search (Google, Bing, etc)" },
  { value: "reddit", label: "Reddit" },
  { value: "hackernews", label: "Hackernews" },
  { value: "ai-chatbots", label: "AI Chatbots (ChatGPT, Claude, Perplexity, etc.)" },
  { value: "blog-post", label: "Blog post" },
  { value: "github", label: "Github" },
  { value: "other", label: "Other" },
];

export function PairingToolMultiSelect({ form, showErrors }: { form: any; showErrors: boolean }) {
  const id = useId();
  const [open, setOpen] = useState(false);

  return (
    <form.Field name="pairingTool">
      {(field: any) => {
        const toggleSelection = (value: string) => {
          const newValues =
            field.state.value.includes(value) ?
              field.state.value.filter((v: string) => v !== value)
            : [...field.state.value, value];
          field.handleChange(newValues);
        };

        const removeSelection = (value: string) => {
          field.handleChange(field.state.value.filter((v: string) => v !== value));
        };

        const hasError = showErrors && field.state.value.length === 0;

        return (
          <div className="space-y-2">
            <Label htmlFor={id}>
              What do you use for pairing and chat? <span className="text-red-500">*</span>
            </Label>
            <DialogTrigger isOpen={open} onOpenChange={setOpen}>
              <Pressable>
                <Button
                  type="button"
                  id={id}
                  variant="outline"
                  role="combobox"
                  aria-expanded={open}
                  className="h-auto min-h-10 w-full justify-between hover:bg-transparent"
                >
                  <div className="flex flex-wrap items-center gap-1 pr-2.5">
                    {field.state.value.length > 0 ?
                      <>
                        {field.state.value.map((val: string) => {
                          const tool = pairingTools.find((t) => t.value === val);
                          return tool ?
                              <Badge key={val} variant="outline">
                                {tool.label}
                                <Button
                                  type="button"
                                  variant="ghost"
                                  size="icon"
                                  className="size-4"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    removeSelection(val);
                                  }}
                                  asChild
                                >
                                  <span>
                                    <XIcon className="size-3" />
                                  </span>
                                </Button>
                              </Badge>
                            : null;
                        })}
                      </>
                    : <span className="text-muted-foreground font-normal">Select pairing tools...</span>}
                  </div>
                  <ChevronsUpDownIcon size={16} className="text-muted-foreground/80 shrink-0" aria-hidden="true" />
                </Button>
              </Pressable>
              <Popover
                placement="bottom start"
                offset={8}
                className="w-(--trigger-width) p-0 z-50 max-h-(--popover-content-available-height) bg-popover border border-border rounded-md shadow-md"
              >
                <AriaDialog>
                  <Command>
                    <CommandList>
                      <CommandEmpty>No pairing tool found.</CommandEmpty>
                      <CommandGroup>
                        {pairingTools.map((tool) => (
                          <CommandItem key={tool.value} value={tool.value} onSelect={() => toggleSelection(tool.value)}>
                            <span className="truncate">{tool.label}</span>
                            {field.state.value.includes(tool.value) && <CheckIcon size={16} className="ml-auto" />}
                          </CommandItem>
                        ))}
                      </CommandGroup>
                    </CommandList>
                  </Command>
                </AriaDialog>
              </Popover>
            </DialogTrigger>
            {hasError && <p className="text-sm text-red-500">At least one pairing tool is required</p>}
          </div>
        );
      }}
    </form.Field>
  );
}

export function ReferralSourceSelect({ form, showErrors }: { form: any; showErrors: boolean }) {
  const id = useId();
  const [open, setOpen] = useState(false);

  return (
    <>
      <form.Field name="hearAboutHopp">
        {(field: any) => {
          const selectedOption = referralSources.find((s) => s.value === field.state.value);
          const hasError = showErrors && field.state.value === "";

          return (
            <div className="space-y-2">
              <Label htmlFor={id}>
                How did you hear about Hopp? <span className="text-red-500">*</span>
              </Label>
              <DialogTrigger isOpen={open} onOpenChange={setOpen}>
                <Pressable>
                  <Button
                    type="button"
                    id={id}
                    variant="outline"
                    role="combobox"
                    aria-expanded={open}
                    className="h-10 w-full justify-between hover:bg-transparent"
                  >
                    <span className={selectedOption ? "" : "text-muted-foreground font-normal"}>
                      {selectedOption ? selectedOption.label : "Select an option..."}
                    </span>
                    <ChevronsUpDownIcon size={16} className="text-muted-foreground/80 shrink-0" aria-hidden="true" />
                  </Button>
                </Pressable>
                <Popover
                  placement="bottom start"
                  offset={8}
                  className="w-(--trigger-width) p-0 z-50 max-h-(--popover-content-available-height) bg-popover border border-border rounded-md shadow-md"
                >
                  <AriaDialog>
                    <Command>
                      <CommandList>
                        <CommandEmpty>No referral source found.</CommandEmpty>
                        <CommandGroup>
                          {referralSources.map((source) => (
                            <CommandItem
                              key={source.value}
                              value={source.value}
                              onSelect={() => {
                                field.handleChange(source.value);
                                setOpen(false);
                              }}
                            >
                              <span className="truncate">{source.label}</span>
                              {field.state.value === source.value && <CheckIcon size={16} className="ml-auto" />}
                            </CommandItem>
                          ))}
                        </CommandGroup>
                      </CommandList>
                    </Command>
                  </AriaDialog>
                </Popover>
              </DialogTrigger>
              {hasError && <p className="text-sm text-red-500">This field is required</p>}
            </div>
          );
        }}
      </form.Field>

      <form.Field name="hearAboutHopp">
        {(field: any) => {
          if (field.state.value !== "other") return null;

          return (
            <form.Field name="hearAboutHoppOther">
              {(otherField: any) => {
                const hasError = showErrors && otherField.state.value === "";
                return (
                  <div className="space-y-2">
                    <Label htmlFor="hearAboutHoppOther">Please specify</Label>
                    <Input
                      id="hearAboutHoppOther"
                      value={otherField.state.value}
                      onChange={(e) => otherField.handleChange(e.target.value)}
                      placeholder="How did you hear about Hopp?"
                    />
                    {hasError && <p className="text-sm text-red-500">Please specify how you heard about us</p>}
                  </div>
                );
              }}
            </form.Field>
          );
        }}
      </form.Field>
    </>
  );
}
