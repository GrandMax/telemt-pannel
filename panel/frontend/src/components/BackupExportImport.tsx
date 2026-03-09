import { useMemo, useRef, useState } from "react";
import {
  Alert,
  AlertDescription,
  AlertIcon,
  AlertTitle,
  Box,
  Button,
  FormControl,
  FormLabel,
  HStack,
  Input,
  ListItem,
  Select,
  Text,
  UnorderedList,
  useDisclosure,
  useToast,
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
} from "@chakra-ui/react";
import {
  type ExportSnapshot,
  type ImportReport,
  useExportSnapshot,
  useImportSnapshot,
} from "../api/adminBackup";

function exportFilename(now: Date): string {
  return `telemt-export-${now.toISOString().slice(0, 10).replace(/-/g, "")}.json`;
}

function triggerDownload(snapshot: ExportSnapshot): void {
  const blob = new Blob([JSON.stringify(snapshot, null, 2)], {
    type: "application/json",
  });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = exportFilename(new Date());
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
}

export default function BackupExportImport() {
  const toast = useToast();
  const exportMutation = useExportSnapshot();
  const importMutation = useImportSnapshot();
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [mode, setMode] = useState<"merge" | "replace">("merge");
  const [report, setReport] = useState<ImportReport | null>(null);
  const confirmReplace = useDisclosure();
  const cancelRef = useRef<HTMLButtonElement | null>(null);

  const fileLabel = useMemo(() => {
    if (!selectedFile) {
      return "No file selected";
    }
    return `${selectedFile.name} (${selectedFile.size} bytes)`;
  }, [selectedFile]);

  const handleExport = () => {
    exportMutation.mutate(undefined, {
      onSuccess: (snapshot) => {
        triggerDownload(snapshot);
        toast({
          title: "Export downloaded",
          description: `Saved ${snapshot.users.length} users to JSON snapshot`,
          status: "success",
        });
      },
      onError: (error: Error) => {
        toast({ title: error.message, status: "error" });
      },
    });
  };

  const runImport = async () => {
    if (!selectedFile) {
      toast({ title: "Select a JSON file first", status: "warning" });
      return;
    }

    let snapshot: ExportSnapshot;
    try {
      snapshot = JSON.parse(await selectedFile.text()) as ExportSnapshot;
    } catch {
      toast({ title: "Invalid JSON file", status: "error" });
      return;
    }

    importMutation.mutate(
      { mode, snapshot },
      {
        onSuccess: (result) => {
          setReport(result);
          toast({
            title: "Import finished",
            description: `added=${result.added}, updated=${result.updated}, skipped=${result.skipped.length}`,
            status: "success",
          });
        },
        onError: (error: Error) => {
          toast({ title: error.message, status: "error" });
        },
      }
    );
  };

  const handleImportClick = () => {
    if (mode === "replace") {
      confirmReplace.onOpen();
      return;
    }
    void runImport();
  };

  return (
    <Box bg="white" borderRadius="md" shadow="sm" p={5}>
      <Text fontSize="lg" fontWeight="semibold" mb={2}>
        Backup and restore
      </Text>
      <Text color="gray.600" mb={5}>
        Export a portable JSON snapshot of users and runtime panel settings, or import it back with merge or replace mode.
      </Text>
      <Text color="orange.600" fontSize="sm" mb={5}>
        Imported panel settings are applied to the running panel immediately, but they are not persisted across restarts yet.
      </Text>

      <HStack align="start" spacing={6} flexWrap="wrap">
        <Box minW="280px" flex="1">
          <Text fontWeight="medium" mb={2}>
            Export
          </Text>
          <Text color="gray.600" fontSize="sm" mb={3}>
            Downloads a snapshot from <code>/api/admin/export</code>.
          </Text>
          <Button
            colorScheme="blue"
            onClick={handleExport}
            isLoading={exportMutation.isPending}
          >
            Download settings export
          </Button>
        </Box>

        <Box minW="320px" flex="1">
          <Text fontWeight="medium" mb={2}>
            Import
          </Text>
          <FormControl mb={3}>
            <FormLabel>JSON file</FormLabel>
            <Input
              type="file"
              accept="application/json,.json"
              onChange={(event) => setSelectedFile(event.target.files?.[0] ?? null)}
              p={1}
            />
            <Text mt={2} fontSize="sm" color="gray.600">
              {fileLabel}
            </Text>
          </FormControl>

          <FormControl mb={4}>
            <FormLabel>Import mode</FormLabel>
            <Select
              value={mode}
              onChange={(event) => setMode(event.target.value as "merge" | "replace")}
            >
              <option value="merge">merge</option>
              <option value="replace">replace</option>
            </Select>
          </FormControl>

          <Button
            colorScheme={mode === "replace" ? "red" : "blue"}
            onClick={handleImportClick}
            isLoading={importMutation.isPending}
          >
            Import snapshot
          </Button>
        </Box>
      </HStack>

      {report && (
        <Alert status="info" mt={6} alignItems="start" borderRadius="md">
          <AlertIcon />
          <Box>
            <AlertTitle>Import report</AlertTitle>
            <AlertDescription>
              added={report.added}, updated={report.updated}, skipped={report.skipped.length}
            </AlertDescription>
            {report.skipped.length > 0 && (
              <UnorderedList mt={2}>
                {report.skipped.map((item, index) => (
                  <ListItem key={`${item.username ?? "unknown"}-${index}`}>
                    {(item.username ?? "<unknown>")}: {item.reason}
                  </ListItem>
                ))}
              </UnorderedList>
            )}
          </Box>
        </Alert>
      )}

      <AlertDialog
        isOpen={confirmReplace.isOpen}
        leastDestructiveRef={cancelRef}
        onClose={confirmReplace.onClose}
      >
        <AlertDialogOverlay />
        <AlertDialogContent>
          <AlertDialogHeader>Confirm replace import</AlertDialogHeader>
          <AlertDialogBody>
            Replace mode removes all current users before applying the snapshot. Use it only when you want the imported file to become the single source of truth.
          </AlertDialogBody>
          <AlertDialogFooter>
            <Button ref={cancelRef} onClick={confirmReplace.onClose}>
              Cancel
            </Button>
            <Button
              colorScheme="red"
              ml={3}
              onClick={() => {
                confirmReplace.onClose();
                void runImport();
              }}
            >
              Replace users
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Box>
  );
}
