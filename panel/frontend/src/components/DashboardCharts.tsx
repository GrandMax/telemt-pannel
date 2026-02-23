import {
  Box,
  SimpleGrid,
  Stat,
  StatLabel,
  StatNumber,
  Heading,
  Skeleton,
  Text,
  Table,
  Thead,
  Tbody,
  Tr,
  Th,
  Td,
} from "@chakra-ui/react";
import ReactApexChart from "react-apexcharts";
import { ApexOptions } from "apexcharts";
import { useSystemStats, useTraffic } from "../api/system";
import { useUsersList } from "../api/users";

function formatUptime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

function formatBytes(n: number): string {
  if (n === 0) return "0 B";
  const k = 1024;
  const i = Math.floor(Math.log(n) / Math.log(k));
  return `${(n / Math.pow(k, i)).toFixed(1)} ${["B", "KB", "MB", "GB", "TB"][i]}`;
}

export default function DashboardCharts() {
  const { data: stats, isLoading: statsLoading } = useSystemStats();
  const { data: traffic, isLoading: trafficLoading } = useTraffic(24);
  const { data: usersData } = useUsersList({ offset: 0, limit: 50 });

  const statsCards = (
    <SimpleGrid columns={{ base: 1, md: 3 }} gap={4} mb={6}>
      <Box bg="white" p={4} borderRadius="md" shadow="sm">
        <Stat>
          <StatLabel>Uptime</StatLabel>
          {statsLoading ? (
            <Skeleton height="32px" width="80px" />
          ) : (
            <StatNumber>{stats ? formatUptime(stats.uptime) : "—"}</StatNumber>
          )}
        </Stat>
      </Box>
      <Box bg="white" p={4} borderRadius="md" shadow="sm">
        <Stat>
          <StatLabel>Total connections</StatLabel>
          {statsLoading ? (
            <Skeleton height="32px" width="60px" />
          ) : (
            <StatNumber>{stats?.total_connections ?? "—"}</StatNumber>
          )}
        </Stat>
      </Box>
      <Box bg="white" p={4} borderRadius="md" shadow="sm">
        <Stat>
          <StatLabel>Bad connections</StatLabel>
          {statsLoading ? (
            <Skeleton height="32px" width="60px" />
          ) : (
            <StatNumber>{stats?.bad_connections ?? "—"}</StatNumber>
          )}
        </Stat>
      </Box>
    </SimpleGrid>
  );

  const hourly = traffic?.hourly ?? [];
  const trafficChartOptions: ApexOptions = {
    chart: { type: "area", toolbar: { show: false } },
    xaxis: {
      categories: hourly.map((p) => p.time.slice(0, 13)),
      labels: { rotate: -45 },
    },
    yaxis: {
      labels: { formatter: (v) => formatBytes(v) },
    },
    stroke: { curve: "smooth" },
    fill: { type: "gradient", opacity: 0.4 },
    legend: { position: "top" },
    colors: ["#3182CE", "#38A169"],
  };
  const trafficSeries = [
    {
      name: "From client",
      data: hourly.map((p) => p.octets_from),
    },
    {
      name: "To client",
      data: hourly.map((p) => p.octets_to),
    },
  ];

  const trafficChart = (
    <Box bg="white" p={4} borderRadius="md" shadow="sm" mb={6}>
      <Heading size="sm" mb={3}>
        Traffic (last 24h)
      </Heading>
      {trafficLoading ? (
        <Skeleton height="300px" />
      ) : hourly.length === 0 ? (
        <Text color="gray.500">No traffic data yet.</Text>
      ) : (
        <ReactApexChart
          options={trafficChartOptions}
          series={trafficSeries}
          type="area"
          height={300}
        />
      )}
    </Box>
  );

  const users = usersData?.users ?? [];
  const perUserTable = (
    <Box bg="white" p={4} borderRadius="md" shadow="sm">
      <Heading size="sm" mb={3}>
        Data usage by user
      </Heading>
      <Table size="sm">
        <Thead>
          <Tr>
            <Th>User</Th>
            <Th>Status</Th>
            <Th>Used</Th>
            <Th>Limit</Th>
          </Tr>
        </Thead>
        <Tbody>
          {users.slice(0, 15).map((u) => (
            <Tr key={u.id}>
              <Td>{u.username}</Td>
              <Td>{u.status}</Td>
              <Td>{formatBytes(u.data_used)}</Td>
              <Td>{u.data_limit != null ? formatBytes(u.data_limit) : "—"}</Td>
            </Tr>
          ))}
        </Tbody>
      </Table>
      {users.length === 0 && <Text color="gray.500">No users.</Text>}
    </Box>
  );

  return (
    <>
      {statsCards}
      {trafficChart}
      {perUserTable}
    </>
  );
}
