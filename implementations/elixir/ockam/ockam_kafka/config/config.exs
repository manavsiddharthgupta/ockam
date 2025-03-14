## Application configuration used in release as sys.config or mix run
## THIS CONFIGURATION IS NOT LOADED IF THE APP IS LOADED AS A DEPENDENCY

import Config

config :ockam_kafka,
  endpoints: [{"localhost", 9092}]

config :ockam_hub,
  service_providers: [Ockam.Kafka.Hub.Service.Provider]
