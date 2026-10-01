FROM mcr.microsoft.com/dotnet/sdk:10.0.401 AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends cmake gcc make libc6-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cmake -S ThirdParty/WabiSabi/c -B ThirdParty/WabiSabi/c/build -DCMAKE_BUILD_TYPE=Release && cmake --build ThirdParty/WabiSabi/c/build --parallel 4 && ctest --test-dir ThirdParty/WabiSabi/c/build --output-on-failure
RUN dotnet publish MagicalCryptoWallet.Coordinator -c Release --self-contained false -p:NativeLibraryPath=/src/ThirdParty/WabiSabi/c/build/libwabisabi.so -o /app

FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS runtime
WORKDIR /app
COPY --from=build /app .
ENV ASPNETCORE_URLS=http://+:38126
EXPOSE 38126
USER $APP_UID
ENTRYPOINT ["dotnet", "MagicalCryptoWallet.Coordinator.dll"]
