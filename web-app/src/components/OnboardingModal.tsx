import { useState, useId } from "react";
import { useForm } from "@tanstack/react-form";
import { useAPI } from "@/hooks/useQueryClients";
import { Dialog, DialogBackdrop, DialogPanel, DialogTitle, Description } from "@headlessui/react";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Send, ChevronsUpDownIcon, XIcon, CheckIcon } from "lucide-react";
import CreatableSelect from "react-select/creatable";
import { z } from "zod";
import { toast } from "react-hot-toast";
import { Badge } from "@/components/ui/badge";
import { Command, CommandEmpty, CommandGroup, CommandItem, CommandList } from "@/components/ui/command";
import { Pressable, DialogTrigger, Popover } from "react-aria-components";
import { Dialog as AriaDialog } from "react-aria-components";
import { OnboardingFeatured } from "./ui/on-boarding-featured";

const emailSchema = z.string().email("Invalid email format");

interface EmailOption {
  value: string;
  label: string;
}

interface OnboardingFormData {
  companySize: string;
  pairingTool: string[];
  signUpReason: string;
}

const companySizes = [
  { value: "1-5", label: "1-5" },
  { value: "5-20", label: "5-20" },
  { value: "20-100", label: "20-100" },
  { value: "100+", label: "100+" },
];

const pairingTools = [
  { value: "slack-huddle", label: "Slack (Huddle)" },
  { value: "microsoft-teams", label: "Microsoft Teams" },
  { value: "google-meet", label: "Google Meet" },
  { value: "zoom", label: "Zoom" },
  { value: "tuple", label: "Tuple, Co-screen etc." },
];

interface OnboardingModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

function CompanySizeSelect({ form, showErrors }: { form: any; showErrors: boolean }) {
  const id = useId();
  const [open, setOpen] = useState(false);

  return (
    <form.Field name="companySize">
      {(field: any) => {
        const selectedOption = companySizes.find((s) => s.value === field.state.value);
        const hasError = showErrors && field.state.value === "";

        return (
          <div className="space-y-2">
            <Label htmlFor={id}>
              How big is your company's engineering department? <span className="text-red-500">*</span>
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
                    {selectedOption ? selectedOption.label : "Select company size..."}
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
                      <CommandEmpty>No company size found.</CommandEmpty>
                      <CommandGroup>
                        {companySizes.map((size) => (
                          <CommandItem
                            key={size.value}
                            value={size.value}
                            onSelect={() => {
                              field.handleChange(size.value);
                              setOpen(false);
                            }}
                          >
                            <span className="truncate">{size.label}</span>
                            {field.state.value === size.value && <CheckIcon size={16} className="ml-auto" />}
                          </CommandItem>
                        ))}
                      </CommandGroup>
                    </CommandList>
                  </Command>
                </AriaDialog>
              </Popover>
            </DialogTrigger>
            {hasError && <p className="text-sm text-red-500">Company size is required</p>}
          </div>
        );
      }}
    </form.Field>
  );
}

function PairingToolMultiSelect({ form, showErrors }: { form: any; showErrors: boolean }) {
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

export function OnboardingModal({ open, onOpenChange }: OnboardingModalProps) {
  const { useMutation } = useAPI();
  const [emailOptions, setEmailOptions] = useState<EmailOption[]>([]);
  const [emailError, setEmailError] = useState<string | null>(null);
  const [hasInvited, setHasInvited] = useState(false);
  const [showValidationErrors, setShowValidationErrors] = useState(false);

  const { mutateAsync: updateOnboardingFormStatus, isPending: isSubmitting } = useMutation(
    "post",
    "/api/auth/metadata/onboarding-form",
  ) as any;
  const { mutateAsync: inviteTeammates, isPending: isInviting } = useMutation(
    "post",
    "/api/auth/send-team-invites",
  ) as any;

  const form = useForm({
    defaultValues: {
      companySize: "",
      pairingTool: [] as string[],
      signUpReason: "",
    },
    onSubmit: async ({ value }: { value: OnboardingFormData }) => {
      if (value.companySize === "" || value.pairingTool.length === 0) {
        setShowValidationErrors(true);
        toast.error("Please fill in all required fields");
        return;
      }

      console.log("Will submit onboarding data", value);
      try {
        await updateOnboardingFormStatus({
          body: {
            onboarding: {
              companySize: value.companySize,
              pairingTool: value.pairingTool,
              signUpReason: value.signUpReason,
              invited: hasInvited,
            },
          },
        });
        onOpenChange(false);
        toast.success("Onboarding completed!");
      } catch (error) {
        toast.error("Failed to submit onboarding form");
        console.error(error);
      }
    },
  });

  const validateEmail = (email: string): boolean => {
    try {
      emailSchema.parse(email);
      return true;
    } catch {
      return false;
    }
  };

  const handleCreateOption = (inputValue: string) => {
    setEmailError(null);

    if (!validateEmail(inputValue)) {
      setEmailError("Invalid email format");
      return;
    }

    if (emailOptions.some((option) => option.value === inputValue)) {
      setEmailError("Email already added");
      return;
    }

    const newOption = { value: inputValue, label: inputValue };
    setEmailOptions([...emailOptions, newOption]);
  };

  const handleInviteUsers = async () => {
    if (emailOptions.length === 0) {
      toast.error("Please add at least one email to invite");
      return;
    }

    try {
      const emails = emailOptions.map((option) => option.value);
      await inviteTeammates({
        body: {
          invitees: emails,
        },
      });

      toast.success(`Invitation sent to ${emails.length} email(s)`);
      setHasInvited(true);
      setEmailOptions([]);
    } catch (error) {
      toast.error("Limit reached, please try inviting your teammates again in a few hours");
      console.error(error);
    }
  };

  const canComplete = hasInvited;

  return (
    <Dialog open={open} onClose={() => {}} className="relative z-50">
      <DialogBackdrop className="fixed inset-0 bg-black/30" transition />
      <div className="fixed inset-0 z-10 w-screen overflow-y-auto">
        <div className="flex min-h-full items-center justify-center p-4">
          <DialogPanel
            transition
            className="w-full max-w-2xl overflow-x-clip max-h-[90vh] overflow-y-auto rounded-2xl p-6 shadow-xl duration-300 ease-out data-closed:transform-[scale(95%)] data-closed:opacity-0 bg-linear-to-b from-[#F5F0FF] via-white via-30% to-white inset-shadow-sm inset-shadow-black/5 ring-2 ring-white border border-slate-200"
            id="onboarding-modal"
          >
            <OnboardingFeatured className="w-[97  %] mx-auto h-auto mb-4" />
            <DialogTitle as="h2" className="h3 text-2xl font-semibold">
              Pair. Ship. Repeat.
            </DialogTitle>
            <Description className="font-normal text-base text-gray-600 mt-2">
              Help us understand your team, then invite your teammates. You'll be the one who brought them the
              collaboration tool that makes remote pairing feel local
            </Description>

            <form
              onSubmit={(e) => {
                e.preventDefault();
                form.handleSubmit();
              }}
              className="space-y-6 mt-6"
            >
              <CompanySizeSelect form={form} showErrors={showValidationErrors} />

              <PairingToolMultiSelect form={form} showErrors={showValidationErrors} />

              <form.Field name="signUpReason">
                {(field) => (
                  <div className="space-y-2">
                    <Label htmlFor="signUpReason">Why did you sign up for Hopp?</Label>
                    <Textarea
                      id="signUpReason"
                      value={field.state.value}
                      onChange={(e) => field.handleChange(e.target.value)}
                      placeholder="Tell us why you signed up..."
                      rows={3}
                    />
                  </div>
                )}
              </form.Field>

              <div className="space-y-2">
                <Label htmlFor="email-invite">Invite your teammates</Label>
                <p className="text-sm text-muted-foreground">
                  Enter email addresses to send invitations directly to your teammates
                </p>
                <div className="flex items-center gap-2">
                  <div className="flex-1">
                    <CreatableSelect
                      id="email-invite"
                      isMulti
                      placeholder="Type email addresses and press enter..."
                      options={[]}
                      value={emailOptions}
                      onChange={(newValue) => {
                        const options = (newValue as EmailOption[]) || [];
                        setEmailOptions(options);
                      }}
                      onCreateOption={handleCreateOption}
                      formatCreateLabel={(inputValue) => `Add "${inputValue}"`}
                      classNamePrefix="react-select"
                      className="react-select-container"
                      components={{
                        DropdownIndicator: () => null,
                        IndicatorSeparator: () => null,
                      }}
                      styles={{
                        control: (base) => ({
                          ...base,
                          fontSize: "12px",
                        }),
                      }}
                    />
                  </div>
                  <Button
                    type="button"
                    onClick={handleInviteUsers}
                    disabled={emailOptions.length === 0 || isInviting}
                    variant="outline"
                    size="icon"
                    className="shrink-0"
                  >
                    <Send className="h-4 w-4" />
                  </Button>
                </div>
                {emailError && <p className="text-red-500 text-xs mt-1">{emailError}</p>}
                {hasInvited && <p className="text-sm text-green-600">Invitations sent successfully!</p>}
              </div>

              <form.Subscribe
                selector={(state) => ({
                  companySize: state.values.companySize,
                  pairingTool: state.values.pairingTool,
                })}
              >
                {({ companySize, pairingTool }) => {
                  const isPartiallyFilled = companySize !== "" && pairingTool.length > 0;

                  return (
                    <div className="w-full flex items-start justify-end ml-auto pt-4 mt-4 gap-2">
                      {isPartiallyFilled && !hasInvited && (
                        <Button
                          type="button"
                          variant="ghost"
                          onClick={() => {
                            form.handleSubmit();
                          }}
                          className="text-muted-foreground cursor-pointer"
                        >
                          Submit without inviting
                        </Button>
                      )}
                      <Button type="submit" disabled={!canComplete || isSubmitting}>
                        {isSubmitting ? "Submitting..." : "Complete"}
                      </Button>
                    </div>
                  );
                }}
              </form.Subscribe>
            </form>
          </DialogPanel>
        </div>
      </div>
    </Dialog>
  );
}
