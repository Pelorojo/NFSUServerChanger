#include "stdafx.h"
#include "stdio.h"
#include <string>
#include <windows.h>
#include "includes\injector\injector.hpp"
#include "includes\IniReader.h"

int Init()
{

	CIniReader iniReader("NFSUServerChanger.ini");

	// gameserver
	char* defaultSrv = "ps2nfs04.ea.com";
	char* gameServer = iniReader.ReadString("Server", "Host", defaultSrv);
	if (strcmp(gameServer, defaultSrv) != 0) {
		typedef void* memory_pointer_tr;
		uintptr_t address = 0x6F1F40;
		memory_pointer_tr memAddr = reinterpret_cast<memory_pointer_tr>(address);
		injector::WriteMemoryRaw(memAddr, gameServer, strlen(gameServer) + 1, true);
	}

	return 0;
}

BOOL APIENTRY DllMain(HMODULE /*hModule*/, DWORD reason, LPVOID /*lpReserved*/)
{
	if (reason == DLL_PROCESS_ATTACH)
	{
		uintptr_t base = (uintptr_t)GetModuleHandleA(NULL);
		IMAGE_DOS_HEADER* dos = (IMAGE_DOS_HEADER*)(base);
		IMAGE_NT_HEADERS* nt = (IMAGE_NT_HEADERS*)(base + dos->e_lfanew);

		if ((base + nt->OptionalHeader.AddressOfEntryPoint + (0x400000 - base)) == 0x670CB5) // Check if .exe file is compatible - Thanks to thelink2012 and MWisBest
			Init();

		else
		{
			MessageBoxA(NULL, "This .exe is not supported.\nPlease use v1.4 English speed.exe (3,03 MB (3.178.496 bytes)).", "NFSU Server Changer", MB_ICONERROR);
			return FALSE;
		}
	}
	return TRUE;

}

