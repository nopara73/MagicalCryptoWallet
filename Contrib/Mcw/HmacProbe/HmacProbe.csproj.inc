<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <IsPackable>false</IsPackable>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
    <DisableImplicitNamespaceImports>true</DisableImplicitNamespaceImports>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="$(HmacCoreProject)" />
    <Compile Include="$(HmacHostSource)" Link="ProductionHost.cs" />
    <Compile Include="$(HmacProbeSourceRoot)/Program.cs" Link="Program.cs" />
    <Compile Include="$(HmacProbeSourceRoot)/TransportFaults.cs" Link="TransportFaults.cs" />
  </ItemGroup>
</Project>
