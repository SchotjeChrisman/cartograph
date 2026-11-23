import { Dialog, DialogBackdrop, DialogClose, DialogContent, DialogDescription, DialogTitle } from "@ui-sdk/react/dialog"

/** Shows a failure from the server until dismissed; nothing is retried behind it. */
export function ErrorDialog({ message, onClose }) {
  return (
    <Dialog open={message !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogBackdrop />
      <DialogContent>
        <DialogTitle>Something went wrong</DialogTitle>
        <DialogDescription>{message}</DialogDescription>
        <DialogClose>Close</DialogClose>
      </DialogContent>
    </Dialog>
  )
}
