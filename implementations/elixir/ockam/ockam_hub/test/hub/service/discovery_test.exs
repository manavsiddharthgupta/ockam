defmodule Test.Hub.Service.DiscoveryTest do
  use ExUnit.Case

  alias Ockam.CloudApi.DiscoveryClient

  alias Ockam.Hub.Service.Discovery
  alias Ockam.Hub.Service.Discovery.Storage

  test "in-memory register and list" do
    {:ok, pid, _address} =
      Discovery.start_link(
        address: "discovery_memory",
        storage: Storage.Memory
      )

    DiscoveryClient.register_service(["discovery_memory"], "discovered_service", ["me"], %{})

    {:ok, services} = DiscoveryClient.list_services([], ["discovery_memory"])

    assert [%{id: "discovered_service", route: ["me"]}] = services
  end

  test "supervisor list" do
    supervisor = Test.Hub.Service.DiscoveryTest.Supervisor
    {:ok, sup_pid} = Supervisor.start_link([], name: supervisor, strategy: :one_for_one)

    {:ok, pid, _address} =
      Discovery.start_link(
        address: "discovery_supervisor",
        storage: Storage.Supervisor,
        storage_options: [supervisor: supervisor]
      )

    Supervisor.start_child(
      supervisor,
      Supervisor.child_spec(
        {Test.Hub.Service.DiscoveryTest.Service, [address: "discovered_service"]},
        id: :discovered_service
      )
    )

    {:ok, services} = DiscoveryClient.list_services([], ["discovery_supervisor"])

    assert [%{id: "discovered_service", route: ["discovered_service"]}] = services
    ## on_exit happens on a different process
    ## causing the test process to get a shutdown form the supervisor
    ## unlink to avoid error message
    Process.unlink(sup_pid)
  end
end

defmodule Test.Hub.Service.DiscoveryTest.Service do
  use Ockam.Worker

  @impl true
  def handle_message(_message, state) do
    {:ok, state}
  end
end
