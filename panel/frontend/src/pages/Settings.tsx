import { Alert, AlertDescription, AlertIcon, AlertTitle, Box, Heading, Text } from "@chakra-ui/react";
import BackupExportImport from "../components/BackupExportImport";
import { useMe } from "../api/auth";

export default function Settings() {
  const { data: me, isLoading } = useMe();

  if (isLoading) {
    return <Text>Loading settings…</Text>;
  }

  if (!me?.is_sudo) {
    return (
      <Alert status="warning" borderRadius="md">
        <AlertIcon />
        <Box>
          <AlertTitle>Administrator access required</AlertTitle>
          <AlertDescription>
            Backup export/import is available only for sudo administrators.
          </AlertDescription>
        </Box>
      </Alert>
    );
  }

  return (
    <Box>
      <Heading size="lg" mb={4}>
        Settings
      </Heading>
      <Text color="gray.600" mb={5}>
        Administrative tools for backup and restore of panel users and runtime settings.
      </Text>
      <Alert status="info" borderRadius="md" mb={5}>
        <AlertIcon />
        <Box>
          <AlertTitle>Runtime-only settings restore</AlertTitle>
          <AlertDescription>
            Imported panel settings take effect immediately in the running process, but they are not persisted across panel restarts yet.
          </AlertDescription>
        </Box>
      </Alert>
      <BackupExportImport />
    </Box>
  );
}
