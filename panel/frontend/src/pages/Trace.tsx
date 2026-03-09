import { useMemo, useState } from "react";
import {
  Alert,
  AlertDescription,
  AlertIcon,
  AlertTitle,
  Badge,
  Box,
  Grid,
  GridItem,
  Heading,
  Input,
  Select,
  Table,
  Tbody,
  Td,
  Text,
  Th,
  Thead,
  Tr,
} from "@chakra-ui/react";
import { useMe } from "../api/auth";
import { useTraceSession, useTraceSessions } from "../api/adminTrace";

function formatTimestamp(value: number | null | undefined): string {
  if (!value) return "—";
  return new Date(value).toLocaleString();
}

export default function Trace() {
  const { data: me, isLoading } = useMe();
  const [userFilter, setUserFilter] = useState("");
  const [dcFilter, setDcFilter] = useState("");
  const [selectedConnId, setSelectedConnId] = useState<number | null>(null);

  const sessionsQuery = useTraceSessions({
    user: userFilter || undefined,
    dc: dcFilter || undefined,
    limit: 50,
  });
  const sessions = sessionsQuery.data ?? [];

  const traceQuery = useTraceSession(selectedConnId, 200);
  const selectedSession = traceQuery.data;

  const dcOptions = useMemo(() => {
    const dcs = new Set<number>();
    for (const session of sessions) {
      dcs.add(session.target_dc);
    }
    return Array.from(dcs).sort((a, b) => a - b);
  }, [sessions]);

  if (isLoading) {
    return <Text>Loading trace tools…</Text>;
  }

  if (!me?.is_sudo) {
    return (
      <Alert status="warning" borderRadius="md">
        <AlertIcon />
        <Box>
          <AlertTitle>Administrator access required</AlertTitle>
          <AlertDescription>
            Trace inspection is available only for sudo administrators.
          </AlertDescription>
        </Box>
      </Alert>
    );
  }

  return (
    <Box>
      <Heading size="lg" mb={4}>
        Trace
      </Heading>
      <Text color="gray.600" mb={5}>
        Inspect active and recently closed Middle-End sessions together with their in-memory trace events.
      </Text>

      <Grid templateColumns={{ base: "1fr", xl: "420px 1fr" }} gap={6}>
        <GridItem>
          <Box bg="white" borderRadius="md" shadow="sm" p={4}>
            <Text fontWeight="semibold" mb={3}>
              Sessions
            </Text>
            <Input
              placeholder="Filter by user"
              value={userFilter}
              onChange={(event) => setUserFilter(event.target.value)}
              mb={3}
            />
            <Select
              placeholder="Filter by DC"
              value={dcFilter}
              onChange={(event) => setDcFilter(event.target.value)}
              mb={4}
            >
              {dcOptions.map((dc) => (
                <option key={dc} value={String(dc)}>
                  DC {dc}
                </option>
              ))}
            </Select>

            <Box maxH="70vh" overflowY="auto">
              <Table size="sm">
                <Thead>
                  <Tr>
                    <Th>User</Th>
                    <Th>DC</Th>
                    <Th>State</Th>
                    <Th>Events</Th>
                  </Tr>
                </Thead>
                <Tbody>
                  {sessions.map((session) => (
                    <Tr
                      key={session.conn_id}
                      cursor="pointer"
                      bg={selectedConnId === session.conn_id ? "blue.50" : undefined}
                      onClick={() => setSelectedConnId(session.conn_id)}
                    >
                      <Td>{session.user}</Td>
                      <Td>{session.target_dc}</Td>
                      <Td>
                        <Badge colorScheme={session.state === "active" ? "green" : "orange"}>
                          {session.state}
                        </Badge>
                      </Td>
                      <Td>{session.event_count}</Td>
                    </Tr>
                  ))}
                </Tbody>
              </Table>
              {sessions.length === 0 && (
                <Text color="gray.500" mt={4}>
                  No trace sessions found for current filters.
                </Text>
              )}
            </Box>
          </Box>
        </GridItem>

        <GridItem>
          <Box bg="white" borderRadius="md" shadow="sm" p={4} minH="70vh">
            <Text fontWeight="semibold" mb={3}>
              Session detail
            </Text>
            {!selectedConnId && (
              <Text color="gray.500">Select a session from the left to view its trace log.</Text>
            )}
            {selectedSession && (
              <>
                <Text mb={2}>
                  <strong>Connection:</strong> {selectedSession.conn_id}
                </Text>
                <Text mb={2}>
                  <strong>User:</strong> {selectedSession.user}
                </Text>
                <Text mb={2}>
                  <strong>DC:</strong> {selectedSession.target_dc}
                </Text>
                <Text mb={2}>
                  <strong>Client:</strong> {selectedSession.client_addr}
                </Text>
                <Text mb={2}>
                  <strong>Our addr:</strong> {selectedSession.our_addr}
                </Text>
                <Text mb={2}>
                  <strong>State:</strong> {selectedSession.state}
                </Text>
                <Text mb={4}>
                  <strong>Closed:</strong> {formatTimestamp(selectedSession.closed_at_ms)}
                </Text>

                <Box
                  as="pre"
                  bg="gray.900"
                  color="gray.100"
                  borderRadius="md"
                  p={4}
                  overflowX="auto"
                  whiteSpace="pre-wrap"
                  fontSize="sm"
                >
                  {selectedSession.events.length === 0
                    ? "No trace events recorded."
                    : selectedSession.events
                        .map(
                          (event) =>
                            `[${formatTimestamp(event.timestamp_ms)}] #${event.seq} ${event.kind} ${event.message}`
                        )
                        .join("\n")}
                </Box>
              </>
            )}
          </Box>
        </GridItem>
      </Grid>
    </Box>
  );
}
